package vault

import (
	"context"
	"encoding/json"
	"errors"
	"strings"
	"testing"
)

func TestMetadataDeletionRollsBackDocumentsAndTombstones(t *testing.T) {
	for _, mode := range []string{"stale_snapshot_from_other_connection", "second_delete_storage_error"} {
		t.Run(mode, func(t *testing.T) {
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
			if err := m.CommitDocuments(ctx, Mutation{Key: "capture/a", Value: json.RawMessage(`{"bytes":"a"}`)}, Mutation{Key: "capture/b", Value: json.RawMessage(`{"bytes":"b"}`)}); err != nil {
				t.Fatal(err)
			}
			a, err := m.ReadDocument(ctx, "capture/a")
			if err != nil {
				t.Fatal(err)
			}
			b, err := m.ReadDocument(ctx, "capture/b")
			if err != nil {
				t.Fatal(err)
			}
			other, err := s.OpenMetadata(ctx)
			if err != nil {
				t.Fatal(err)
			}
			defer other.Close()
			wantB := b
			if mode == "stale_snapshot_from_other_connection" {
				if err := other.CommitDocuments(ctx, Mutation{Key: b.Key, ExpectedGeneration: b.Generation, Value: json.RawMessage(`{"bytes":"newer"}`)}); err != nil {
					t.Fatal(err)
				}
				wantB, err = other.ReadDocument(ctx, b.Key)
				if err != nil {
					t.Fatal(err)
				}
			} else {
				// Abort the second DELETE only after the first document and the
				// second tombstone have been changed inside the transaction.
				if _, err := m.db.ExecContext(ctx, `CREATE TRIGGER fail_second_delete BEFORE DELETE ON documents WHEN OLD.key='capture/b' BEGIN SELECT RAISE(ABORT, 'injected second delete failure'); END;`); err != nil {
					t.Fatal(err)
				}
			}
			err = m.DeleteDocuments(ctx, Deletion{Key: a.Key, ExpectedGeneration: a.Generation}, Deletion{Key: b.Key, ExpectedGeneration: b.Generation})
			if mode == "stale_snapshot_from_other_connection" {
				if !errors.Is(err, ErrGeneration) {
					t.Fatalf("stale deletion: %v", err)
				}
			} else if err == nil || !strings.Contains(err.Error(), "injected second delete failure") {
				t.Fatalf("missing storage failure: %v", err)
			}
			for _, want := range []Document{a, wantB} {
				got, err := other.ReadDocument(ctx, want.Key)
				if err != nil || got.Generation != want.Generation || string(got.Value) != string(want.Value) {
					t.Fatalf("partial deletion visible to another connection: got=%+v want=%+v err=%v", got, want, err)
				}
			}
			var tombstones int
			if err := other.db.QueryRowContext(ctx, "SELECT count(*) FROM deleted_generations").Scan(&tombstones); err != nil || tombstones != 0 {
				t.Fatalf("rolled-back removal retained tombstones: %d %v", tombstones, err)
			}
			if mode == "second_delete_storage_error" {
				if _, err := m.db.ExecContext(ctx, "DROP TRIGGER fail_second_delete"); err != nil {
					t.Fatal(err)
				}
			}
			if err := m.DeleteDocuments(ctx, Deletion{Key: a.Key, ExpectedGeneration: a.Generation}, Deletion{Key: wantB.Key, ExpectedGeneration: wantB.Generation}); err != nil {
				t.Fatal(err)
			}
			for _, old := range []Document{a, wantB} {
				if _, err := other.ReadDocument(ctx, old.Key); !errors.Is(err, ErrMissingRecord) {
					t.Fatalf("successful retry left %s: %v", old.Key, err)
				}
				if err := other.CommitDocuments(ctx, Mutation{Key: old.Key, Value: old.Value}); err != nil {
					t.Fatal(err)
				}
				got, err := m.ReadDocument(ctx, old.Key)
				if err != nil || got.Generation != old.Generation+1 {
					t.Fatalf("retry lost monotonic generation: %+v %v", got, err)
				}
				if err := m.DeleteDocuments(ctx, Deletion{Key: old.Key, ExpectedGeneration: old.Generation}); !errors.Is(err, ErrGeneration) {
					t.Fatalf("stale deletion authorized recreated %s: %v", old.Key, err)
				}
			}
		})
	}
}
