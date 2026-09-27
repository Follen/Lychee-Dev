package codebase

import (
	"context"
	"errors"
	"path/filepath"
	"testing"

	"github.com/follenfang/lycheedev/internal/vault"
)

func TestPrepareSourceSelectionErrorsAreTypedBeforeFetch(t *testing.T) {
	ctx := context.Background()
	store, err := vault.Initialize(ctx, filepath.Join(t.TempDir(), "home"))
	if err != nil {
		t.Fatal(err)
	}
	b := OpenBrowser(store)
	for _, test := range []struct {
		repo, product, ref string
		want               error
	}{
		{"missing-repo", "main", "", ErrUnknownRepository},
		{"weakauras", "retail", "", ErrUnknownProduct},
		{"weakauras", "main", "main", ErrInvalidSourceRef},
		{"weakauras", "main", "refs/heads/bad ref", ErrInvalidSourceRef},
	} {
		_, err := b.PrepareSource(ctx, test.repo, test.product, test.ref)
		if !errors.Is(err, test.want) {
			t.Errorf("PrepareSource(%q,%q,%q) error=%v; want %v", test.repo, test.product, test.ref, err, test.want)
		}
	}
}
