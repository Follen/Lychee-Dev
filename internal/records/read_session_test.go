package records

import (
	"bytes"
	"context"
	"errors"
	"os"
	"path/filepath"
	"testing"

	"github.com/follenfang/lycheedev/internal/records/container"
	"github.com/follenfang/lycheedev/internal/records/resource"
	"github.com/follenfang/lycheedev/internal/vault"
)

func sessionTestReader(t *testing.T, q FileQuery) (*Reader, string) {
	t.Helper()
	workspace := filepath.Join(t.TempDir(), "workspace")
	store, err := vault.Initialize(context.Background(), workspace)
	if err != nil {
		t.Fatal(err)
	}
	reader, err := OpenReader(store).NewSession(context.Background(), validReaderPin(), q)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := reader.Close(); err != nil {
			t.Error(err)
		}
	})
	return reader, workspace
}

func TestReadSessionBindsCapturedKeyProvider(t *testing.T) {
	provider := func(value byte) container.KeyLookup {
		return func(context.Context, uint64) ([]byte, error) { return []byte{value}, nil }
	}
	q := FileQuery{Installation: "must-not-be-opened", FileDataID: 11, MetadataBytes: 1 << 20, ContentBytes: 1 << 20, Keys: provider(1)}
	reader, _ := sessionTestReader(t, q)
	bound := reader.Query()
	if bound.Keys != nil || reader.session.validate(validReaderPin(), bound) != nil {
		t.Fatal("session did not own and bind its original key provider")
	}
	for name, altered := range map[string]FileQuery{
		"other closure from same call site": func() FileQuery { changed := bound; changed.Keys = provider(2); return changed }(),
		"original options without binding":  func() FileQuery { changed := q; changed.Keys = nil; return changed }(),
		"foreign session binding":           func() FileQuery { changed := bound; changed.binding = &readSession{}; return changed }(),
	} {
		t.Run(name, func(t *testing.T) {
			if _, err := reader.ReadFile(context.Background(), validReaderPin(), altered); !errors.Is(err, ErrFileQuery) {
				t.Fatalf("substituted key source reached I/O: %v", err)
			}
		})
	}
}

func TestReadSessionCloseReleasesHeavyReferencesOnce(t *testing.T) {
	reader, _ := sessionTestReader(t, FileQuery{Keys: func(context.Context, uint64) ([]byte, error) { return nil, nil }})
	s := reader.session
	indexClosed, sourceClosed := 0, 0
	s.closeIndex = func() { indexClosed++ }
	s.source.done = func() { sourceClosed++ }
	s.rootBytes = make([]byte, 1024)
	s.rootIndex = &rootIndex{}
	s.index = &EncodingIndex{}
	s.keys = s.query.Keys
	s.keySource = &KeySource{}
	s.hotfix = &hotfixSnapshots{captures: map[string]*hotfixSnapshot{"capture": {raw: make([]byte, 1024)}}}
	s.views["table"] = preparedSessionTable{}
	s.root = vault.BlobRef{Bytes: 1024}
	s.rootStats.Hits = 1
	reader.verifiedBytes = make([]byte, 1024)
	reader.verifiedBlob = vault.BlobRef{Bytes: 1024}
	reader.blobStats = &DecodedCacheStats{}
	for i := 0; i < 2; i++ {
		if err := reader.Close(); err != nil {
			t.Fatal(err)
		}
	}
	if indexClosed != 1 || sourceClosed != 1 || !s.closed || s.admission != nil || s.rootBytes != nil || s.rootIndex != nil || s.index != nil || s.keys != nil || s.keySource != nil || s.hotfix != nil || s.views != nil || s.closeIndex != nil || s.source.done != nil || s.source.open != nil || s.query.Keys != nil || s.query.budget != nil || s.root.Bytes != 0 || s.rootStats.Hits != 0 || reader.verifiedBytes != nil || reader.verifiedBlob.Bytes != 0 || reader.blobStats != nil {
		t.Fatal("closed session retained resources or closed a handle twice")
	}
	if err := reader.CheckSource(context.Background()); !errors.Is(err, ErrFileQuery) {
		t.Fatal(err)
	}
}

func TestReadSessionVerifiedBlobBudgetAndQueryBoundary(t *testing.T) {
	limits := resource.DefaultLimits()
	limits.RetainedBytes = 4
	reader, workspace := sessionTestReader(t, FileQuery{budget: resource.New(limits)})
	ctx := context.Background()
	ref, err := reader.store.PublishBlob(ctx, vault.BlobInput{Reader: bytes.NewReader([]byte("data")), MaxBytes: 4})
	if err != nil {
		t.Fatal(err)
	}
	stats := &DecodedCacheStats{}
	reader.blobStats = stats
	first, err := reader.readVerifiedBlob(ctx, ref, 4)
	if err != nil {
		t.Fatal(err)
	}
	second, err := reader.readVerifiedBlob(ctx, ref, 4)
	if err != nil || &first[0] != &second[0] || stats.BlobReads != 1 || reader.session.query.budget.Snapshot().RetainedBytes != 4 {
		t.Fatalf("same-query consumer duplicated allocation/read: %v %+v", err, stats)
	}
	if err := reader.Close(); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(workspace, "blobs", ref.SHA256[:2], ref.SHA256[2:])
	if err := os.WriteFile(path, []byte("evil"), 0600); err != nil {
		t.Fatal(err)
	}
	// Authentication belongs to one query. A new session must not reuse the
	// previous query's authenticated bytes after same-size cache corruption.
	other, err := OpenReader(reader.store).NewSession(ctx, validReaderPin(), FileQuery{budget: resource.New(limits)})
	if err != nil {
		t.Fatal(err)
	}
	defer other.Close()
	if _, err := other.readVerifiedBlob(ctx, ref, 4); !errors.Is(err, vault.ErrBlobIntegrity) || other.verifiedBytes != nil {
		t.Fatalf("new query reused corrupt cache: %v", err)
	}
	limits.RetainedBytes = 3
	bounded, err := OpenReader(reader.store).NewSession(ctx, validReaderPin(), FileQuery{budget: resource.New(limits)})
	if err != nil {
		t.Fatal(err)
	}
	defer bounded.Close()
	// Resource failure takes precedence over the corrupted blob, proving the
	// retained allocation reservation precedes the cached physical read.
	if _, err := bounded.readVerifiedBlob(ctx, ref, 4); !errors.Is(err, resource.ErrBudget) || bounded.verifiedBytes != nil {
		t.Fatalf("cache read allocated before query budget: %v", err)
	}
}
