package records

import (
	"bytes"
	"context"
	"errors"
	"os"
	"path/filepath"
	"testing"

	"github.com/follenfang/lycheedev/internal/records/container"
	"github.com/follenfang/lycheedev/internal/vault"
)

func TestKeySourcesAreExplicitBoundedAndOffline(t *testing.T) {
	ctx := context.Background()
	root := filepath.Join(t.TempDir(), "workspace")
	if _, err := vault.Initialize(ctx, root); err != nil {
		t.Fatal(err)
	}
	_, err := vault.WriteMetadata(ctx, root, func(store *vault.Store, _ *vault.Metadata) (bool, error) {
		r := OpenReader(store)
		lookup, source, err := r.prepareKeys(ctx, FileQuery{Offline: true})
		if err != nil {
			return false, err
		}
		if source.State != "not_needed" {
			t.Fatal(source)
		}
		if _, err := lookup(ctx, 1); !errors.Is(err, container.ErrKeyUnavailable) || source.State != "unavailable_offline" {
			t.Fatalf("offline miss: %v %+v", err, source)
		}
		for _, format := range []string{"text", "json"} {
			path := filepath.Join(t.TempDir(), "keys."+format)
			raw := "0000000000000001 000102030405060708090a0b0c0d0e0f"
			if format == "json" {
				raw = `{"0000000000000001":"000102030405060708090a0b0c0d0e0f"}`
			}
			if err := os.WriteFile(path, []byte(raw), 0600); err != nil {
				t.Fatal(err)
			}
			get, info, err := r.prepareKeys(ctx, FileQuery{Offline: true, KeyFile: path})
			if err != nil {
				t.Fatal(err)
			}
			key, err := get(ctx, 1)
			if err != nil || len(key) != 16 || key[15] != 15 || info.Kind != "file" || len(info.SHA256) != 64 {
				t.Fatalf("key source: %v %+v", err, info)
			}
			if err := os.WriteFile(path, []byte("private invalid material"), 0600); err != nil {
				t.Fatal(err)
			}
			if _, _, err := r.prepareKeys(ctx, FileQuery{KeyFile: path}); !errors.Is(err, ErrKeyDocument) {
				t.Fatalf("bad document: %v", err)
			}
			// This request owns the previous immutable key snapshot, even if the file changes.
			if key, err := get(ctx, 1); err != nil || len(key) != 16 {
				t.Fatal(err)
			}
		}
		return true, nil
	})
	if err != nil {
		t.Fatal(err)
	}
}

func TestMissingBytesCannotBecomeZeroValues(t *testing.T) {
	r := availableReader{ReaderAt: bytes.NewReader([]byte{1, 2, 0, 0, 5}), missing: []container.MissingSpan{{Offset: 2, Bytes: 2, KeyID: "0000000000000001"}}}
	for _, offset := range []int64{1, 2, 3} {
		if _, err := r.ReadAt(make([]byte, 2), offset); !errors.Is(err, container.ErrKeyUnavailable) {
			t.Fatalf("gap exposed at %d: %v", offset, err)
		}
	}
	if n, err := r.ReadAt(make([]byte, 2), 0); err != nil || n != 2 {
		t.Fatal(n, err)
	}
}
