package evidence

import (
	"context"
	"encoding/json"
	"errors"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/vault"
)

func TestCapturePersistsExactBytesAndPartialStatus(t *testing.T) {
	ctx := context.Background()
	s, err := vault.Initialize(ctx, filepath.Join(t.TempDir(), "new"))
	if err != nil {
		t.Fatal(err)
	}
	m, err := s.OpenMetadata(ctx)
	if err != nil {
		t.Fatal(err)
	}
	a := OpenArchive(s, m)
	const raw = "{\"result\": \"中文\"}\r\n"
	ref, err := a.CommitCapture(ctx, CaptureDraft{Reader: strings.NewReader(raw), MaxBytes: 1024, MediaType: "application/json", Provenance: Provenance{Kind: "probe", Locator: "request-1", Snapshot: "PIN-fixed", OperationID: "OP-fixed"}, Truncated: true})
	if err != nil {
		t.Fatal(err)
	}
	if err := m.Close(); err != nil {
		t.Fatal(err)
	}
	m, err = s.OpenMetadata(ctx)
	if err != nil {
		t.Fatal(err)
	}
	defer m.Close()
	a = OpenArchive(s, m)
	observed, data, err := a.FetchCapture(ctx, ref.ID, 1024)
	if err != nil || string(data) != raw || observed.Complete || !observed.Truncated || observed.Provenance != ref.Provenance {
		t.Fatalf("%+v %q %v", observed, data, err)
	}
	if err := a.VerifyCapture(ctx, ref.ID); err != nil {
		t.Fatal(err)
	}
	page, err := a.ListCaptures(ctx, "", 10)
	if err != nil || len(page) != 1 || page[0].ID != ref.ID {
		t.Fatalf("%+v %v", page, err)
	}
	next, err := a.ListCaptures(ctx, ref.ID, 10)
	if err != nil || len(next) != 0 {
		t.Fatalf("%+v %v", next, err)
	}
}

// A capture key digests the whole manifest, so two committers that produce the
// same manifest race on one insert. This is the CI-exposed shape of concurrent
// archive reads: one process must reuse the committed object instead of failing
// with vault.generation_conflict. The pinned clock makes the collision
// deterministic, without sleeps or scheduler assumptions.
func TestCaptureCommitReusesIdenticalContentAddressedObject(t *testing.T) {
	ctx := context.Background()
	s, err := vault.Initialize(ctx, filepath.Join(t.TempDir(), "new"))
	if err != nil {
		t.Fatal(err)
	}
	m, err := s.OpenMetadata(ctx)
	if err != nil {
		t.Fatal(err)
	}
	defer m.Close()
	a := OpenArchive(s, m)

	frozen := time.Date(2026, 9, 23, 8, 0, 0, 0, time.UTC)
	capturedAt = func() time.Time { return frozen }
	t.Cleanup(func() { capturedAt = func() time.Time { return time.Now().UTC() } })

	// Every commit consumes its reader, so each attempt gets a fresh identical one.
	newDraft := func() CaptureDraft {
		return CaptureDraft{Reader: strings.NewReader(`{"page":1}`), MaxBytes: 1024, MediaType: "application/json", Complete: true, Provenance: Provenance{Kind: "hotfix-records", Locator: "dbcache:sha256:abc", Snapshot: "PIN-fixed", OperationID: "OP-fixed"}}
	}
	first, err := a.CommitCapture(ctx, newDraft())
	if err != nil {
		t.Fatal(err)
	}
	second, err := a.CommitCapture(ctx, newDraft())
	if err != nil {
		t.Fatalf("identical re-commit failed the insert CAS: %v", err)
	}
	if second != first {
		t.Fatalf("reuse returned a different object: %+v != %+v", second, first)
	}

	// Concurrent committers of the same content-addressed object all succeed.
	const racers = 8
	refs := make([]CaptureRef, racers)
	failures := make(chan error, racers)
	var wg sync.WaitGroup
	for i := 0; i < racers; i++ {
		wg.Add(1)
		go func(i int) {
			defer wg.Done()
			ref, err := a.CommitCapture(ctx, newDraft())
			refs[i] = ref
			if err != nil {
				failures <- err
			}
		}(i)
	}
	wg.Wait()
	close(failures)
	for err := range failures {
		t.Fatal(err)
	}
	for _, ref := range refs {
		if ref != first {
			t.Fatalf("racer returned a different object: %+v != %+v", ref, first)
		}
	}

	// Diverging bytes under the same key remain a conflict, never a silent
	// overwrite or an accepted mismatch.
	doc, err := m.ReadDocument(ctx, "capture/"+first.ID)
	if err != nil {
		t.Fatal(err)
	}
	if err := m.CommitDocuments(ctx, vault.Mutation{Key: "capture/" + first.ID, ExpectedGeneration: doc.Generation, Value: json.RawMessage(`{"schema":"lycheedev.capture.v1"}`)}); err != nil {
		t.Fatal(err)
	}
	if _, err := a.CommitCapture(ctx, newDraft()); !errors.Is(err, vault.ErrGeneration) {
		t.Fatalf("diverging content under one content address: %v", err)
	}
}
