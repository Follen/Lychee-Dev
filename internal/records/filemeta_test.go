package records_test

import (
	"context"
	"crypto/md5"
	"encoding/hex"
	"path/filepath"
	"testing"

	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/records"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/testkit"
	"github.com/follenfang/lycheedev/internal/vault"
)

func workspaceRoot(t *testing.T) string {
	t.Helper()
	root := filepath.Join(t.TempDir(), "workspace")
	if _, err := vault.Initialize(context.Background(), root); err != nil {
		t.Fatal(err)
	}
	return root
}

func cachedAssetPin(t *testing.T, workspace string, body []byte) selection.PinnedSet {
	t.Helper()
	return testkit.CachedAsset(t, workspace, body)
}

func cdnQuery(fileDataID uint32) records.FileQuery {
	return records.FileQuery{CDN: true, Offline: true, FileDataID: fileDataID, MetadataBytes: 1 << 20, ContentBytes: 1 << 20}
}

func md5hex(raw []byte) string {
	digest := md5.Sum(raw)
	return hex.EncodeToString(digest[:])
}

func openEvidence(t *testing.T, workspace string) *evidence.Archive {
	t.Helper()
	ctx := context.Background()
	store, err := vault.OpenStore(workspace)
	if err != nil {
		t.Fatal(err)
	}
	metadata, err := store.OpenMetadata(ctx)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { metadata.Close() })
	return evidence.OpenArchive(store, metadata)
}
