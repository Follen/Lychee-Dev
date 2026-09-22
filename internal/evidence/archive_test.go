package evidence

import (
	"context"
	"path/filepath"
	"strings"
	"testing"

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
