package vault

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"testing"
)

func TestReadMetadataNeverInitializesAndRejectsWrites(t *testing.T) {
	ctx := context.Background()
	s, err := Initialize(ctx, filepath.Join(t.TempDir(), "workspace"))
	if err != nil {
		t.Fatal(err)
	}
	if _, err := s.ReadMetadata(ctx); !errors.Is(err, ErrMissingRecord) {
		t.Fatal(err)
	}
	for _, dir := range []string{"state", "locks"} {
		entries, err := os.ReadDir(filepath.Join(s.Root(), dir))
		if err != nil || len(entries) != 0 {
			t.Fatalf("read changed %s: %v %v", dir, entries, err)
		}
	}
	w, err := s.OpenMetadata(ctx)
	if err != nil {
		t.Fatal(err)
	}
	defer w.Close()
	if err := w.CommitDocuments(ctx, Mutation{Key: "test/read", Value: json.RawMessage(`true`)}); err != nil {
		t.Fatal(err)
	}
	r, err := s.ReadMetadata(ctx)
	if err != nil {
		t.Fatal(err)
	}
	defer r.Close()
	doc, err := r.ReadDocument(ctx, "test/read")
	if err != nil || string(doc.Value) != "true" {
		t.Fatalf("read: %+v %v", doc, err)
	}
	if err := r.CommitDocuments(ctx, Mutation{Key: "test/read", ExpectedGeneration: 1, Value: json.RawMessage(`false`)}); err == nil {
		t.Fatal("read-only connection wrote")
	}
	if err := w.CommitDocuments(ctx, Mutation{Key: "test/read", ExpectedGeneration: 1, Value: json.RawMessage(`false`)}); err != nil {
		t.Fatal(err)
	}
	doc, err = r.ReadDocument(ctx, "test/read")
	if err != nil || doc.Generation != 2 || string(doc.Value) != "false" {
		t.Fatalf("live writer not observed: %+v %v", doc, err)
	}
}
