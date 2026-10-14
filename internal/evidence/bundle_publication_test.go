package evidence

import (
	"archive/zip"
	"context"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"

	"github.com/follenfang/lycheedev/internal/vault"
)

// Complete ZIP bytes make this a publication race, rather than the existing
// preflight-conflict or cancellation cases. No sleep or production hook is used.
type bundleCollisionContext struct {
	context.Context
	output  string
	once    sync.Once
	created bool
	err     error
}

func (c *bundleCollisionContext) Err() error {
	stages, err := filepath.Glob(filepath.Join(filepath.Dir(c.output), ".lycheedev-bundle-*"))
	if err != nil {
		return err
	}
	for _, stage := range stages {
		r, err := zip.OpenReader(stage)
		if err != nil {
			continue
		}
		complete := len(r.File) > 0 && r.File[len(r.File)-1].Name == "manifest.json"
		r.Close()
		if complete {
			c.once.Do(func() {
				c.err = os.WriteFile(c.output, []byte("competing export must survive"), 0600)
				c.created = c.err == nil
			})
		}
	}
	if c.err != nil {
		return c.err
	}
	return c.Context.Err()
}

func TestBundleLateOutputCollisionPreservesBothExports(t *testing.T) {
	base := context.Background()
	s, err := vault.Initialize(base, t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	m, err := s.OpenMetadata(base)
	if err != nil {
		t.Fatal(err)
	}
	defer m.Close()
	a := OpenArchive(s, m)
	const payload = "exact source evidence\r\n"
	ref, err := a.CommitCapture(base, CaptureDraft{Reader: strings.NewReader(payload), MaxBytes: 1024, MediaType: "text/plain", Complete: true, Provenance: Provenance{Kind: "fixture", Locator: "late-output-collision"}})
	if err != nil {
		t.Fatal(err)
	}
	output := filepath.Join(t.TempDir(), "evidence.zip")
	ctx := &bundleCollisionContext{Context: base, output: output}
	manifest, err := Bundle(ctx, s.Root(), []string{ref.ID}, output)
	if !ctx.created || !errors.Is(err, ErrBundleOutput) || manifest.Complete {
		t.Fatalf("late collision: created=%v result=%+v err=%v", ctx.created, manifest, err)
	}
	raw, err := os.ReadFile(output)
	if err != nil || string(raw) != "competing export must survive" {
		t.Fatalf("competing output replaced: %q %v", raw, err)
	}
	if stages, err := filepath.Glob(filepath.Join(filepath.Dir(output), ".lycheedev-bundle-*")); err != nil || len(stages) != 0 {
		t.Fatalf("stage residue: %v %v", stages, err)
	}
	_, raw, err = a.FetchCapture(base, ref.ID, 1024)
	if err != nil || string(raw) != payload {
		t.Fatalf("source changed after conflict: %q %v", raw, err)
	}
	// A new destination can still publish exactly this capture after the race.
	retry := filepath.Join(filepath.Dir(output), "retry.zip")
	if got, err := Bundle(base, s.Root(), []string{ref.ID}, retry); err != nil || !got.Complete || got.Captures[0].ID != ref.ID {
		t.Fatalf("retry: %+v %v", got, err)
	}
}
