package vault

import (
	"context"
	"encoding/json"
	"errors"
	"path/filepath"
	"sync"
	"sync/atomic"
	"testing"
)

func TestMetadataTransactionsAndPersistence(t *testing.T) {
	ctx := context.Background()
	s, err := Initialize(ctx, filepath.Join(t.TempDir(), "new"))
	if err != nil {
		t.Fatal(err)
	}
	m, err := s.OpenMetadata(ctx)
	if err != nil {
		t.Fatal(err)
	}
	if err := m.CommitDocuments(ctx, Mutation{Key: "test/a", Value: json.RawMessage(`{"n":1}`)}); err != nil {
		t.Fatal(err)
	}
	if err := m.CommitDocuments(ctx, Mutation{Key: "test/b", Value: json.RawMessage(`true`)}, Mutation{Key: "test/a", ExpectedGeneration: 9, Value: json.RawMessage(`false`)}); !errors.Is(err, ErrGeneration) {
		t.Fatal(err)
	}
	if _, err := m.ReadDocument(ctx, "test/b"); !errors.Is(err, ErrMissingRecord) {
		t.Fatalf("partial commit: %v", err)
	}
	if err := m.Close(); err != nil {
		t.Fatal(err)
	}
	m, err = s.OpenMetadata(ctx)
	if err != nil {
		t.Fatal(err)
	}
	defer m.Close()
	doc, err := m.ReadDocument(ctx, "test/a")
	if err != nil || doc.Generation != 1 || string(doc.Value) != `{"n":1}` {
		t.Fatalf("%+v %v", doc, err)
	}
	if err := m.CommitDocuments(ctx, Mutation{Key: doc.Key, ExpectedGeneration: 1, Value: json.RawMessage(`{"n":2}`)}); err != nil {
		t.Fatal(err)
	}
	page, err := m.ListDocuments(ctx, "test/", "", 10)
	if err != nil || len(page) != 1 || page[0].Generation != 2 {
		t.Fatalf("%+v %v", page, err)
	}
}

func TestIndependentMetadataConnectionsCompete(t *testing.T) {
	ctx := context.Background()
	s, err := Initialize(ctx, filepath.Join(t.TempDir(), "new"))
	if err != nil {
		t.Fatal(err)
	}
	var group sync.WaitGroup
	var winners atomic.Int32
	for range 8 {
		group.Go(func() {
			m, err := s.OpenMetadata(ctx)
			if err != nil {
				t.Error(err)
				return
			}
			defer m.Close()
			err = m.CommitDocuments(ctx, Mutation{Key: "shared", Value: json.RawMessage(`true`)})
			if err == nil {
				winners.Add(1)
			} else if !errors.Is(err, ErrGeneration) {
				t.Error(err)
			}
		})
	}
	group.Wait()
	if winners.Load() != 1 {
		t.Fatalf("winners %d", winners.Load())
	}
}

func TestMetadataFutureSchemaNotDowngraded(t *testing.T) {
	ctx := context.Background()
	s, err := Initialize(ctx, filepath.Join(t.TempDir(), "new"))
	if err != nil {
		t.Fatal(err)
	}
	m, err := s.OpenMetadata(ctx)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := m.db.ExecContext(ctx, "PRAGMA user_version=99"); err != nil {
		t.Fatal(err)
	}
	if err := m.Close(); err != nil {
		t.Fatal(err)
	}
	if other, err := s.OpenMetadata(ctx); !errors.Is(err, ErrWorkspaceFormat) {
		if other != nil {
			other.Close()
		}
		t.Fatalf("future schema: %v", err)
	}
}

func TestDeletedGenerationCannotAuthorizeReplacement(t *testing.T) {
	ctx := context.Background()
	s, err := Initialize(ctx, t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	m, err := s.OpenMetadata(ctx)
	if err != nil {
		t.Fatal(err)
	}
	defer m.Close()
	change := Mutation{Key: "target/one", Value: json.RawMessage(`true`)}
	if err := m.CommitDocuments(ctx, change); err != nil {
		t.Fatal(err)
	}
	old, _ := m.ReadDocument(ctx, change.Key)
	if err := m.DeleteDocuments(ctx, Deletion{Key: old.Key, ExpectedGeneration: old.Generation}); err != nil {
		t.Fatal(err)
	}
	if err := m.CommitDocuments(ctx, change); err != nil {
		t.Fatal(err)
	}
	change.ExpectedGeneration = old.Generation
	if err := m.CommitDocuments(ctx, change); !errors.Is(err, ErrGeneration) {
		t.Fatalf("stale replace: %v", err)
	}
	if err := m.DeleteDocuments(ctx, Deletion{Key: old.Key, ExpectedGeneration: old.Generation}); !errors.Is(err, ErrGeneration) {
		t.Fatalf("stale delete: %v", err)
	}
}
