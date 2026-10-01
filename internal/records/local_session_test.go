package records

import (
	"context"
	"crypto/sha256"
	"errors"
	"os"
	"path/filepath"
	"testing"
)

func TestLocalSessionRevalidatesObservedPathsAndGenerations(t *testing.T) {
	for _, mode := range []string{"same", "same-size-edit", "new-generation", "cancel"} {
		t.Run(mode, func(t *testing.T) {
			ctx := context.Background()
			root := t.TempDir()
			dir := filepath.Join(root, "Data", "data")
			if err := os.MkdirAll(dir, 0700); err != nil {
				t.Fatal(err)
			}
			path := filepath.Join(dir, "0000000001.idx")
			raw := []byte("observed immutable index content")
			if err := os.WriteFile(path, raw, 0600); err != nil {
				t.Fatal(err)
			}
			session, err := openLocalSession(ctx, root, nil)
			if err != nil {
				t.Fatal(err)
			}
			defer session.close()
			// Inject the digest already established by authenticated OpenIndex;
			// this isolates the path revalidation boundary from parser fixtures.
			session.observed["0000000001.idx"] = localIndexSnapshot{digest: sha256.Sum256(raw), size: int64(len(raw))}
			var want error
			switch mode {
			case "same-size-edit":
				raw[0] ^= 1
				if err := os.WriteFile(path, raw, 0600); err != nil {
					t.Fatal(err)
				}
				want = ErrPinnedBuildChanged
			case "new-generation":
				if err := os.WriteFile(filepath.Join(dir, "0000000002.idx"), raw, 0600); err != nil {
					t.Fatal(err)
				}
				want = ErrPinnedBuildChanged
			case "cancel":
				var cancel context.CancelFunc
				ctx, cancel = context.WithCancel(ctx)
				cancel()
				want = context.Canceled
			}
			if err := session.verify(ctx); !errors.Is(err, want) {
				t.Fatal(err, want)
			}
		})
	}
}
