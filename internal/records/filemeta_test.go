package records_test

import (
	"bytes"
	"context"
	"crypto/md5"
	"encoding/hex"
	"encoding/json"
	"errors"
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

func TestInspectFileEncodingMetadataOnly(t *testing.T) {
	ctx := context.Background()
	body := []byte("payload whose decoded content must never be fetched twice")
	workspace := workspaceRoot(t)
	pin := cachedAssetPin(t, workspace, body)
	reading, err := records.InspectFileEncoding(ctx, workspace, pin.ID, records.FileEncodingRequest{
		File: cdnQuery(11), FileDataID: 11,
	})
	if err != nil {
		t.Fatal(err)
	}
	result := reading.Result
	if result.ContentKey != md5hex(body) || result.RootEntry.FileDataID != 11 || result.RootEntry.ContentKey != result.ContentKey {
		t.Fatalf("content identity = %+v", result)
	}
	if result.DecodedBytes != int64(len(body)) {
		t.Fatalf("decoded bytes = %d, want %d", result.DecodedBytes, len(body))
	}
	// The synthetic fixture stores one framed BLTE object per content key.
	if len(result.EncodingKeys) != 1 || len(result.Encoded) != 1 {
		t.Fatalf("encoding keys = %+v", result)
	}
	if result.Encoded[0].EncodingKey != result.EncodingKeys[0] || result.Encoded[0].EncodedBytes != int64(len(body))+9 {
		t.Fatalf("encoded record = %+v", result.Encoded)
	}
	if result.Context.Source != "cdn" || result.Context.Snapshot != pin.ID || result.Context.Pin != *pin.Data {
		t.Fatalf("context = %+v", result.Context)
	}
	if len(result.Archives) != 0 {
		t.Fatalf("CDN reading must not invent archive locations: %+v", result.Archives)
	}
	// The reading is archived as evidence with verifiable bytes.
	archive := openEvidence(t, workspace)
	captured, raw, err := archive.FetchCapture(ctx, reading.Capture.ID, 1<<20)
	if err != nil {
		t.Fatal(err)
	}
	if captured.Provenance.Kind != "file-encoding" || captured.Provenance.Snapshot != pin.ID {
		t.Fatalf("capture provenance = %+v", captured)
	}
	encoded, err := json.Marshal(result)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(raw, encoded) {
		t.Fatalf("capture bytes diverge from result:\n%s\n%s", raw, encoded)
	}
	if err := archive.VerifyCapture(ctx, reading.Capture.ID); err != nil {
		t.Fatal(err)
	}
	if _, err := records.InspectFileEncoding(ctx, workspace, pin.ID, records.FileEncodingRequest{File: cdnQuery(12), FileDataID: 12}); !errors.Is(err, records.ErrContentMissing) {
		t.Fatalf("missing file: %v", err)
	}
	if _, err := records.InspectFileEncoding(ctx, workspace, pin.ID, records.FileEncodingRequest{File: cdnQuery(11)}); !errors.Is(err, records.ErrFileQuery) {
		t.Fatalf("missing id: %v", err)
	}
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

func TestInspectFileExistenceDistinguishesOutcomes(t *testing.T) {
	ctx := context.Background()
	workspace := workspaceRoot(t)
	pin := cachedAssetPin(t, workspace, []byte("hello"))
	listfile := records.ListfileRequest{
		Kind: records.ListfileCommunityCSV,
		Fetch: records.ListfileFetchFunc(func(ctx context.Context, url string, maxBytes int64) ([]byte, error) {
			return []byte("11;Interface\\Icons\\Test.blp\n12;missing/file.blp\n13;dup.blp\n14;dup.blp\n"), nil
		}),
	}
	// Listed and present in CASC Root.
	present, err := records.InspectFileExistence(ctx, workspace, pin.ID, records.FileExistenceRequest{
		File: cdnQuery(11), Listfile: listfile, FileDataID: 11,
	})
	if err != nil {
		t.Fatal(err)
	}
	if present.Result.ListfileState != records.ListfileStateListed || present.Result.ContentState != records.ContentStatePresent ||
		!present.Result.Exists || !present.Result.LocaleMatched || present.Result.FileName != "interface/icons/test.blp" {
		t.Fatalf("present = %+v", present.Result)
	}
	// Listed but absent in CASC Root: a different outcome than "unlisted".
	absent, err := records.InspectFileExistence(ctx, workspace, pin.ID, records.FileExistenceRequest{
		File: cdnQuery(12), Listfile: listfile, FileDataID: 12,
	})
	if err != nil {
		t.Fatal(err)
	}
	if absent.Result.ListfileState != records.ListfileStateListed || absent.Result.ContentState != records.ContentStateAbsent || absent.Result.Exists {
		t.Fatalf("absent = %+v", absent.Result)
	}
	// Unlisted: no listfile entry at all, still checked against CASC.
	unlisted, err := records.InspectFileExistence(ctx, workspace, pin.ID, records.FileExistenceRequest{
		File: cdnQuery(99), Listfile: listfile, FileDataID: 99,
	})
	if err != nil {
		t.Fatal(err)
	}
	if unlisted.Result.ListfileState != records.ListfileStateUnlisted || unlisted.Result.ContentState != records.ContentStateAbsent {
		t.Fatalf("unlisted = %+v", unlisted.Result)
	}
	// Name queries resolve case-insensitively and normalize separators.
	byName, err := records.InspectFileExistence(ctx, workspace, pin.ID, records.FileExistenceRequest{
		File: cdnQuery(11), Listfile: listfile, FileName: `Interface\Icons\TEST.blp`,
	})
	if err != nil {
		t.Fatal(err)
	}
	if !byName.Result.Exists || len(byName.Result.Matches) != 1 || byName.Result.Matches[0].FileDataID != 11 {
		t.Fatalf("by name = %+v", byName.Result)
	}
	// Multi-match names report every ID; nothing is picked silently.
	duplicated, err := records.InspectFileExistence(ctx, workspace, pin.ID, records.FileExistenceRequest{
		File: cdnQuery(13), Listfile: listfile, FileName: "dup.blp",
	})
	if err != nil {
		t.Fatal(err)
	}
	if len(duplicated.Result.Matches) != 2 || duplicated.Result.Matches[0].FileDataID != 13 || duplicated.Result.Matches[1].FileDataID != 14 {
		t.Fatalf("multi-match = %+v", duplicated.Result.Matches)
	}
	if duplicated.Result.Exists || duplicated.Result.ContentState != records.ContentStateAbsent {
		t.Fatalf("multi-match states = %+v", duplicated.Result)
	}
	// A name without any listing cannot make a CASC claim.
	unknown, err := records.InspectFileExistence(ctx, workspace, pin.ID, records.FileExistenceRequest{
		File: cdnQuery(11), Listfile: listfile, FileName: "nothing/here.blp",
	})
	if err != nil {
		t.Fatal(err)
	}
	if unknown.Result.ListfileState != records.ListfileStateUnlisted || unknown.Result.ContentState != records.ContentStateUnverified || unknown.Result.Exists {
		t.Fatalf("unknown name = %+v", unknown.Result)
	}
	if _, err := records.InspectFileExistence(ctx, workspace, pin.ID, records.FileExistenceRequest{File: cdnQuery(11), Listfile: listfile, FileDataID: 11, FileName: "both.blp"}); !errors.Is(err, records.ErrFileQuery) {
		t.Fatalf("ambiguous request: %v", err)
	}
	// Source unavailable (an uncached source offline) is its own precise
	// outcome, distinct from "unlisted" and "absent".
	offline := records.ListfileRequest{Kind: records.ListfileWowExportText, Offline: true}
	if _, err := records.InspectFileExistence(ctx, workspace, pin.ID, records.FileExistenceRequest{
		File: cdnQuery(11), Listfile: offline, FileDataID: 11,
	}); !errors.Is(err, records.ErrListfileUnavailable) {
		t.Fatalf("source unavailable: %v", err)
	}
	archive := openEvidence(t, workspace)
	if err := archive.VerifyCapture(ctx, present.Capture.ID); err != nil {
		t.Fatal(err)
	}
}
