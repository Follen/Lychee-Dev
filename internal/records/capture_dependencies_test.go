package records_test

import (
	"bytes"
	"context"
	"encoding/binary"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"sync"
	"testing"

	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/records"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/testkit"
	"github.com/follenfang/lycheedev/internal/vault"
)

func dependencyImageFixture() []byte {
	raw := make([]byte, 148)
	copy(raw, "BLP2")
	binary.LittleEndian.PutUint32(raw[4:], 1)
	raw[8], raw[9] = 3, 8
	binary.LittleEndian.PutUint32(raw[12:], 1)
	binary.LittleEndian.PutUint32(raw[16:], 1)
	binary.LittleEndian.PutUint32(raw[20:], 148)
	binary.LittleEndian.PutUint32(raw[84:], 4)
	return append(raw, 0, 0, 255, 255)
}

func ordinaryHotfixFixture(t *testing.T) (string, selection.PinnedSet, records.HotfixRequest) {
	t.Helper()
	ctx := context.Background()
	root := workspaceRoot(t)
	pin, err := vault.WriteMetadata(ctx, root, func(_ *vault.Store, m *vault.Metadata) (selection.PinnedSet, error) {
		return selection.OpenPinner(m).PinSelection(ctx, selection.SelectionSpec{Data: &selection.DataPin{
			Product: "retail", Region: "cn", Language: "zhCN", FullBuild: "12.1.0.69875",
			BuildConfig: strings.Repeat("a", 32), CDNConfig: strings.Repeat("b", 32), DefinitionCommit: strings.Repeat("c", 40),
		}})
	})
	if err != nil {
		t.Fatal(err)
	}
	raw := makeCacheSnapshot(9, 69875, 9,
		cacheRecordFixture{Table: 0xabcdef01, Record: 1, Status: 2},
		cacheRecordFixture{Table: 0xabcdef01, Record: 2, Status: 2},
		cacheRecordFixture{Table: 0xabcdef01, Record: 3, Status: 2},
	)
	path := filepath.Join(t.TempDir(), "DBCache.bin")
	if err := os.WriteFile(path, raw, 0600); err != nil {
		t.Fatal(err)
	}
	return root, pin, records.HotfixRequest{File: path, Filter: records.CacheFilter{Limit: 1}, MaxBytes: 1 << 20}
}

// Reproduce the pre-dependency capture encoding around the exact production
// payload. No identity or bytes of the original capture are rewritten.
func legacyAggregateCapture(t *testing.T, a *evidence.Archive, original evidence.CaptureRef) evidence.CaptureRef {
	t.Helper()
	ctx := context.Background()
	_, raw, err := a.FetchCapture(ctx, original.ID, 32<<20)
	if err != nil {
		t.Fatal(err)
	}
	old, err := a.CommitCapture(ctx, evidence.CaptureDraft{
		Reader: bytes.NewReader(raw), MaxBytes: 32 << 20, MediaType: original.MediaType,
		Provenance: original.Provenance, Complete: original.Complete, Truncated: original.Truncated,
	})
	if err != nil {
		t.Fatal(err)
	}
	return old
}

func requireCaptureOwned(t *testing.T, a *evidence.Archive, ids ...string) {
	t.Helper()
	for _, id := range ids {
		if _, err := a.RemoveCapture(context.Background(), id); !errors.Is(err, evidence.ErrCaptureReferenced) {
			t.Fatalf("owned capture %s removal = %v", id, err)
		}
		if err := a.VerifyCapture(context.Background(), id); err != nil {
			t.Fatalf("owned capture %s lost bytes: %v", id, err)
		}
	}
}

func removeAggregateCapture(t *testing.T, a *evidence.Archive, id string) {
	t.Helper()
	if removed, err := a.RemoveCapture(context.Background(), id); err != nil || !removed.ManifestRemoved {
		t.Fatalf("released capture %s removal = %+v, %v", id, removed, err)
	}
}

func TestHotfixOrdinaryPagesProtectSourceAndContinue(t *testing.T) {
	for _, legacy := range []bool{false, true} {
		t.Run(map[bool]string{false: "current", true: "legacy"}[legacy], func(t *testing.T) {
			ctx := context.Background()
			root, pin, request := ordinaryHotfixFixture(t)
			first, err := records.InspectHotfix(ctx, root, pin.ID, request)
			if err != nil || first.Result.Page.NextIndex == nil || !first.Result.Page.Truncated {
				t.Fatalf("first page = %+v, %v", first, err)
			}
			a := openEvidence(t, root)
			if !reflect.DeepEqual(first.Capture.Dependencies, []string{first.Result.Source.ID}) {
				t.Fatalf("page dependencies = %v", first.Capture.Dependencies)
			}
			if legacy {
				old := legacyAggregateCapture(t, a, first.Capture)
				removeAggregateCapture(t, a, first.Capture.ID)
				first.Capture = old
			}
			_, original, err := a.FetchCapture(ctx, first.Capture.ID, 32<<20)
			if err != nil {
				t.Fatal(err)
			}
			if _, err := a.KeepCapture(ctx, first.Capture.ID); err != nil {
				t.Fatal(err)
			}
			requireCaptureOwned(t, a, first.Result.Source.ID)
			request.File, request.From = "", first.Result.Source.ID
			request.Filter.AfterIndex = first.Result.Page.NextIndex
			second, err := records.InspectHotfix(ctx, root, first.Result.Snapshot, request)
			if err != nil || len(second.Result.Page.Entries) != 1 || second.Result.Page.Entries[0].RecordID != 2 || second.Result.Source.ID != first.Result.Source.ID {
				t.Fatalf("ordinary archived continuation = %+v, %v", second, err)
			}
			ref, body, err := a.FetchCapture(ctx, first.Capture.ID, 32<<20)
			if err != nil || !reflect.DeepEqual(ref, first.Capture) || !bytes.Equal(body, original) {
				t.Fatalf("original page changed during continuation: %v", err)
			}
			removeAggregateCapture(t, a, first.Capture.ID)
			requireCaptureOwned(t, a, first.Result.Source.ID)
			removeAggregateCapture(t, a, second.Capture.ID)
			removeAggregateCapture(t, a, first.Result.Source.ID)
		})
	}
}

func TestAssetExportCapturesRetainOriginalAndEncodedBytes(t *testing.T) {
	for _, encoding := range []string{"raw", "png"} {
		t.Run(encoding, func(t *testing.T) {
			ctx := context.Background()
			root := hotfixTestWorkspace(t)
			raw := dependencyImageFixture()
			pin := testkit.CachedAsset(t, root, raw)
			exported, err := records.ExportAsset(ctx, root, pin.ID, records.AssetExportRequest{
				Output: filepath.Join(t.TempDir(), "image."+encoding), Encoding: encoding,
				File: records.FileQuery{CDN: true, Offline: true, FileDataID: 11, MetadataBytes: 1 << 20, ContentBytes: 1 << 20},
			})
			if err != nil {
				t.Fatal(err)
			}
			a := openEvidence(t, root)
			ids := []string{exported.Manifest.SourceCapture.ID}
			if encoding != "raw" {
				ids = append(ids, exported.Artifact.ID)
				if !reflect.DeepEqual(exported.Artifact.Dependencies, ids[:1]) {
					t.Fatalf("image dependencies = %v", exported.Artifact.Dependencies)
				}
			}
			if !reflect.DeepEqual(exported.Capture.Dependencies, ids) {
				t.Fatalf("export dependencies = %v, want %v", exported.Capture.Dependencies, ids)
			}
			if _, err := a.KeepCapture(ctx, exported.Capture.ID); err != nil {
				t.Fatal(err)
			}
			requireCaptureOwned(t, a, ids...)
			old := legacyAggregateCapture(t, a, exported.Capture)
			removeAggregateCapture(t, a, exported.Capture.ID)
			requireCaptureOwned(t, a, ids...)
			_, source, err := a.FetchCapture(ctx, ids[0], 1<<20)
			if err != nil || !bytes.Equal(source, raw) {
				t.Fatalf("original source bytes = %v", err)
			}
			_, encoded, err := a.FetchCapture(ctx, exported.Artifact.ID, 1<<20)
			published, readErr := os.ReadFile(exported.Manifest.Output)
			if err != nil || readErr != nil || !bytes.Equal(encoded, published) {
				t.Fatalf("encoded artifact differs from output: %v, %v", err, readErr)
			}
			ref, body, err := a.FetchCapture(ctx, old.ID, 1<<20)
			expected, marshalErr := json.Marshal(exported.Manifest)
			if err != nil || marshalErr != nil || !reflect.DeepEqual(ref, old) || !bytes.Equal(body, expected) {
				t.Fatalf("legacy manifest identity changed: %v", err)
			}
			removeAggregateCapture(t, a, old.ID)
			if encoding != "raw" {
				requireCaptureOwned(t, a, ids[0])
				removeAggregateCapture(t, a, exported.Artifact.ID)
			}
			removeAggregateCapture(t, a, ids[0])
		})
	}
}

func TestDemuxManifestRetainsSourceAndFrames(t *testing.T) {
	for _, mode := range []string{"local", "casc", "partial"} {
		t.Run(mode, func(t *testing.T) {
			ctx := context.Background()
			root := hotfixTestWorkspace(t)
			input := filepath.Join("..", "..", "tests", "fixtures", "inputs", "minimal-vp9.avi")
			raw, err := os.ReadFile(input)
			if err != nil {
				t.Fatal(err)
			}
			request := records.AssetDemuxRequest{Path: input, Output: t.TempDir()}
			snapshot := ""
			if mode == "casc" {
				snapshot = testkit.CachedAsset(t, root, raw).ID
				request.Path = ""
				request.File = records.FileQuery{CDN: true, Offline: true, FileDataID: 11, MetadataBytes: 1 << 20, ContentBytes: 1 << 20}
			} else if mode == "partial" {
				request.MaxFrames, request.AllowPartial = 1, true
			}
			demux, err := records.DemuxAsset(ctx, root, snapshot, request)
			if err != nil || demux.Manifest.Complete != (mode != "partial") || demux.Manifest.Truncated != (mode == "partial") {
				t.Fatalf("demux = %+v, %v", demux, err)
			}
			ids := []string{demux.Source.ID}
			for _, frame := range demux.Frames {
				ids = append(ids, frame.ID)
			}
			if !reflect.DeepEqual(demux.Capture.Dependencies, ids[:1]) {
				t.Fatalf("demux source dependency = %v, want %v", demux.Capture.Dependencies, ids[:1])
			}
			a := openEvidence(t, root)
			if _, err := a.KeepCapture(ctx, demux.Capture.ID); err != nil {
				t.Fatal(err)
			}
			requireCaptureOwned(t, a, ids...)
			old := legacyAggregateCapture(t, a, demux.Capture)
			removeAggregateCapture(t, a, demux.Capture.ID)
			requireCaptureOwned(t, a, ids...)
			ref, body, err := a.FetchCapture(ctx, old.ID, 16<<20)
			published, readErr := os.ReadFile(filepath.Join(request.Output, "demux-manifest.json"))
			if err != nil || readErr != nil || !reflect.DeepEqual(ref, old) || !bytes.Equal(body, published) {
				t.Fatalf("legacy manifest changed: %v, %v", err, readErr)
			}
			for i, frame := range demux.Manifest.Frames {
				_, body, err := a.FetchCapture(ctx, frame.Capture, 1<<20)
				published, readErr := os.ReadFile(filepath.Join(request.Output, frame.Output))
				if err != nil || readErr != nil || !bytes.Equal(body, published) {
					t.Fatalf("frame %d bytes differ: %v, %v", i, err, readErr)
				}
			}
			removeAggregateCapture(t, a, old.ID)
			for _, id := range ids {
				removeAggregateCapture(t, a, id)
			}
		})
	}
}

// A root context is consulted for the metadata lease before it is acquired.
// Nested mutations use the lease-bearing context and never reach this hook.
// This injects a real successful removal in the producer gap without changing
// production functions or relying on a racing sleep.
type removeBeforeAggregateContext struct {
	context.Context
	mu      sync.Mutex
	archive *evidence.Archive
	kind    string
	removed string
	err     error
}

func (c *removeBeforeAggregateContext) Value(key any) any {
	if reflect.TypeOf(key).String() == "vault.metadataLeaseKey" {
		c.mu.Lock()
		defer c.mu.Unlock()
		if c.removed == "" && c.err == nil {
			refs, err := c.archive.ListCaptures(context.Background(), "", 1000)
			c.err = err
			for _, ref := range refs {
				if ref.Provenance.Kind == c.kind {
					_, c.err = c.archive.RemoveCapture(context.Background(), ref.ID)
					c.removed = ref.ID
					break
				}
			}
		}
	}
	return c.Context.Value(key)
}

func TestAggregatePublicationRejectsRemovedDependencies(t *testing.T) {
	for _, mode := range []string{"hotfix", "raw-source", "image-source", "image-artifact", "demux-source", "demux-frame"} {
		t.Run(mode, func(t *testing.T) {
			root, pin, hotfix := ordinaryHotfixFixture(t)
			kind := map[string]string{"hotfix": "hotfix-cache", "raw-source": "asset", "image-source": "asset", "image-artifact": "asset-image", "demux-source": "asset-demux-source", "demux-frame": "asset-demux-frame"}[mode]
			ctx := &removeBeforeAggregateContext{Context: context.Background(), archive: openEvidence(t, root), kind: kind}
			output := t.TempDir()
			var err error
			ownerKind := ""
			switch mode {
			case "hotfix":
				ownerKind = "hotfix-records"
				_, err = records.InspectHotfix(ctx, root, pin.ID, hotfix)
			case "raw-source", "image-source", "image-artifact":
				ownerKind = "asset-export-manifest"
				raw := dependencyImageFixture()
				assetPin := testkit.CachedAsset(t, root, raw)
				encoding := "png"
				if mode == "raw-source" {
					encoding = "raw"
				}
				_, err = records.ExportAsset(ctx, root, assetPin.ID, records.AssetExportRequest{Output: filepath.Join(output, "asset"), Encoding: encoding,
					File: records.FileQuery{CDN: true, Offline: true, FileDataID: 11, MetadataBytes: 1 << 20, ContentBytes: 1 << 20}})
			case "demux-source", "demux-frame":
				ownerKind = "asset-demux-manifest"
				_, err = records.DemuxAsset(ctx, root, "", records.AssetDemuxRequest{Path: filepath.Join("..", "..", "tests", "fixtures", "inputs", "minimal-vp9.avi"), Output: output})
			}
			if ctx.err != nil || ctx.removed == "" || !errors.Is(err, vault.ErrMissingRecord) {
				t.Fatalf("publication after removal %s: remove=%v, result=%v", ctx.removed, ctx.err, err)
			}
			refs, listErr := ctx.archive.ListCaptures(context.Background(), "", 1000)
			if listErr != nil {
				t.Fatal(listErr)
			}
			for _, ref := range refs {
				if ref.Provenance.Kind == ownerKind {
					t.Fatalf("published dangling %s capture", ownerKind)
				}
			}
			files, readErr := os.ReadDir(output)
			if readErr != nil || len(files) != 0 {
				t.Fatalf("publication failure wrote external outputs: %v, %v", files, readErr)
			}
		})
	}
}
