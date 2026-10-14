package records

import (
	"bytes"
	"context"
	"errors"
	"reflect"
	"testing"

	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/vault"
)

func removeScanCapture(t *testing.T, root, id string) (evidence.RemoveResult, error) {
	t.Helper()
	ctx := context.Background()
	return vault.WriteMetadata(ctx, root, func(s *vault.Store, m *vault.Metadata) (evidence.RemoveResult, error) {
		return evidence.OpenArchive(s, m).RemoveCapture(ctx, id)
	})
}

func readScanCapture(t *testing.T, root, id string) (evidence.CaptureRef, []byte) {
	t.Helper()
	ctx := context.Background()
	var body []byte
	ref, err := vault.ReadWorkspace(ctx, root, func(s *vault.Store, m *vault.Metadata) (evidence.CaptureRef, error) {
		ref, raw, err := evidence.OpenArchive(s, m).FetchCapture(ctx, id, 32<<20)
		body = raw
		return ref, err
	})
	if err != nil {
		t.Fatalf("read retained capture %s: %v", id, err)
	}
	return ref, body
}

func scanCaptureList(t *testing.T, root string) []evidence.CaptureRef {
	t.Helper()
	ctx := context.Background()
	refs, err := vault.ReadWorkspace(ctx, root, func(s *vault.Store, m *vault.Metadata) ([]evidence.CaptureRef, error) {
		return evidence.OpenArchive(s, m).ListCaptures(ctx, "", 1000)
	})
	if err != nil {
		t.Fatal(err)
	}
	return refs
}

func TestHotfixScanCheckpointsProtectPageCaptures(t *testing.T) {
	ctx := context.Background()
	root, pin, request := hotfixScanFixture(t)
	first, err := ScanHotfix(ctx, root, pin.ID, request, "", 1)
	if err != nil || first.Resume == "" || len(first.Result.Pages) != 1 || first.Result.Complete {
		t.Fatalf("first checkpoint = %+v, %v", first, err)
	}
	pageID := first.Result.Pages[0]
	pageRef, pageBody := readScanCapture(t, root, pageID)
	if _, err := removeScanCapture(t, root, pageID); !errors.Is(err, evidence.ErrCaptureReferenced) {
		t.Fatalf("pending page removal = %v, want capture referenced", err)
	}
	request.File, request.From = "", first.Result.Source.ID
	second, err := ScanHotfix(ctx, root, first.Result.Snapshot, request, first.Resume, 1)
	if err != nil || second.Resume == "" || len(second.Result.Pages) != 2 || second.Result.Pages[0] != pageID {
		t.Fatalf("second checkpoint = %+v, %v", second, err)
	}
	finished, err := ScanHotfix(ctx, root, second.Result.Snapshot, request, second.Resume, 1)
	if err != nil || !finished.Result.Complete || finished.Result.Returned != 3 || len(finished.Result.Pages) != 3 || finished.Resume != "" {
		t.Fatalf("completed checkpoint = %+v, %v", finished, err)
	}
	for _, reading := range []HotfixScanReading{first, second, finished} {
		if !reflect.DeepEqual(reading.Capture.Dependencies, append([]string{reading.Result.Source.ID}, reading.Result.Pages...)) {
			t.Fatalf("checkpoint page ownership not published: %+v", reading.Capture)
		}
	}
	for _, id := range append([]string{finished.Result.Source.ID}, finished.Result.Pages...) {
		if _, err := removeScanCapture(t, root, id); !errors.Is(err, evidence.ErrCaptureReferenced) {
			t.Fatalf("completed dependency %s removal = %v, want capture referenced", id, err)
		}
	}
	before := len(scanCaptureList(t, root))
	again, err := ScanHotfix(ctx, root, finished.Result.Snapshot, request, finished.Capture.ID, 3)
	if err != nil || !reflect.DeepEqual(again, finished) || len(scanCaptureList(t, root)) != before {
		t.Fatalf("complete checkpoint was not reused unchanged: %+v, %v", again, err)
	}
	gotRef, gotBody := readScanCapture(t, root, pageID)
	if !reflect.DeepEqual(gotRef, pageRef) || !bytes.Equal(gotBody, pageBody) {
		t.Fatal("resume or refused removal changed original page evidence")
	}
	// Several immutable checkpoints own the first page. Releasing the completed
	// checkpoint must not release references still held by earlier checkpoints.
	for _, id := range []string{finished.Capture.ID, first.Capture.ID} {
		if _, err := removeScanCapture(t, root, id); err != nil {
			t.Fatal(err)
		}
		if _, err := removeScanCapture(t, root, pageID); !errors.Is(err, evidence.ErrCaptureReferenced) {
			t.Fatalf("shared page reference was released early: %v", err)
		}
	}
	if _, err := removeScanCapture(t, root, second.Capture.ID); err != nil {
		t.Fatal(err)
	}
	for _, id := range append(finished.Result.Pages, finished.Result.Source.ID) {
		removed, err := removeScanCapture(t, root, id)
		if err != nil || !removed.ManifestRemoved || !removed.BlobRemoved {
			t.Fatalf("unreferenced dependency %s cleanup = %+v, %v", id, removed, err)
		}
	}
}

func legacyScanCheckpoint(t *testing.T, root string, original HotfixScanReading, missingPage bool) evidence.CaptureRef {
	t.Helper()
	_, raw := readScanCapture(t, root, original.Capture.ID)
	for _, ref := range scanCaptureList(t, root) {
		if ref.Provenance.Kind == "hotfix-scan" {
			if _, err := removeScanCapture(t, root, ref.ID); err != nil {
				t.Fatal(err)
			}
		}
	}
	if missingPage {
		if _, err := removeScanCapture(t, root, original.Result.Pages[0]); err != nil {
			t.Fatal(err)
		}
	}
	ctx := context.Background()
	ref, err := vault.WriteMetadata(ctx, root, func(s *vault.Store, m *vault.Metadata) (evidence.CaptureRef, error) {
		// This is the pre-upgrade manifest encoding: page IDs are only in the
		// immutable scan blob, and the source uses the original capture: locator.
		return evidence.OpenArchive(s, m).CommitCapture(ctx, evidence.CaptureDraft{
			Reader: bytes.NewReader(raw), MaxBytes: 4 << 20, MediaType: "application/json",
			Complete: original.Result.Complete, Truncated: original.Result.Truncated,
			Provenance: evidence.Provenance{Kind: "hotfix-scan", Locator: "capture:" + original.Result.Source.ID,
				Snapshot: original.Result.Snapshot, DataBuild: original.Result.Source.Provenance.DataBuild},
		})
	})
	if err != nil {
		t.Fatal(err)
	}
	return ref
}

func TestHotfixScanLegacyCheckpointsProtectPagesAndResume(t *testing.T) {
	for _, maxPages := range []int{1, 3} {
		t.Run(map[int]string{1: "pending", 3: "complete"}[maxPages], func(t *testing.T) {
			ctx := context.Background()
			root, pin, request := hotfixScanFixture(t)
			original, err := ScanHotfix(ctx, root, pin.ID, request, "", maxPages)
			if err != nil {
				t.Fatal(err)
			}
			legacy := legacyScanCheckpoint(t, root, original, false)
			_, oldBody := readScanCapture(t, root, legacy.ID)
			for _, id := range append([]string{original.Result.Source.ID}, original.Result.Pages...) {
				if _, err := removeScanCapture(t, root, id); !errors.Is(err, evidence.ErrCaptureReferenced) {
					t.Fatalf("historical dependency %s removal = %v, want capture referenced", id, err)
				}
			}
			request.File, request.From = "", original.Result.Source.ID
			finished, err := ScanHotfix(ctx, root, original.Result.Snapshot, request, legacy.ID, 3)
			if err != nil || !finished.Result.Complete || finished.Result.Returned != 3 || finished.Result.Pages[0] != original.Result.Pages[0] {
				t.Fatalf("historical resume = %+v, %v", finished, err)
			}
			if maxPages == 3 && finished.Capture.ID != legacy.ID {
				t.Fatal("complete historical checkpoint was replaced")
			}
			got, body := readScanCapture(t, root, legacy.ID)
			if !reflect.DeepEqual(got, legacy) || !bytes.Equal(body, oldBody) {
				t.Fatal("historical checkpoint identity or bytes changed")
			}
			for _, id := range finished.Result.Pages {
				readScanCapture(t, root, id)
			}
			for _, ref := range scanCaptureList(t, root) {
				if ref.Provenance.Kind == "hotfix-scan" {
					if _, err := removeScanCapture(t, root, ref.ID); err != nil {
						t.Fatal(err)
					}
				}
			}
			for _, id := range append(finished.Result.Pages, finished.Result.Source.ID) {
				if _, err := removeScanCapture(t, root, id); err != nil {
					t.Fatalf("historical dependency cleanup = %v", err)
				}
			}
		})
	}
}

func TestHotfixScanLegacyMissingPageDoesNotComplete(t *testing.T) {
	for _, maxPages := range []int{1, 3} {
		t.Run(map[int]string{1: "pending", 3: "complete"}[maxPages], func(t *testing.T) {
			ctx := context.Background()
			root, pin, request := hotfixScanFixture(t)
			original, err := ScanHotfix(ctx, root, pin.ID, request, "", maxPages)
			if err != nil {
				t.Fatal(err)
			}
			legacy := legacyScanCheckpoint(t, root, original, true)
			before := len(scanCaptureList(t, root))
			request.File, request.From = "", original.Result.Source.ID
			reading, err := ScanHotfix(ctx, root, original.Result.Snapshot, request, legacy.ID, 3)
			if !errors.Is(err, vault.ErrMissingRecord) || reading.Resume != legacy.ID || reading.Capture.ID != legacy.ID {
				t.Fatalf("damaged historical resume = %+v, %v; want original checkpoint and missing page", reading, err)
			}
			if len(scanCaptureList(t, root)) != before {
				t.Fatal("damaged historical scan published new evidence")
			}
		})
	}
}

func TestHotfixScanPageRemovedBeforeCheckpointPublication(t *testing.T) {
	ctx := context.Background()
	root, pin, request := hotfixScanFixture(t)
	calls := 0
	inspect := func(ctx context.Context, root, snapshot string, q HotfixRequest) (HotfixReading, error) {
		page, err := InspectHotfix(ctx, root, snapshot, q)
		if err != nil {
			return page, err
		}
		calls++
		if calls == 2 {
			if _, err := removeScanCapture(t, root, page.Capture.ID); err != nil {
				t.Fatal(err)
			}
		}
		return page, nil
	}
	reading, err := scanHotfix(ctx, root, pin.ID, request, "", 3, inspect)
	if !errors.Is(err, vault.ErrMissingRecord) || reading.Resume == "" || reading.Resume != reading.Capture.ID || reading.Result.Returned != 1 || len(reading.Result.Pages) != 1 || reading.Result.Complete {
		t.Fatalf("page removed before commit = %+v, %v; want original pending checkpoint", reading, err)
	}
	request.File, request.From = "", reading.Result.Source.ID
	finished, err := ScanHotfix(ctx, root, reading.Result.Snapshot, request, reading.Resume, 3)
	if err != nil || !finished.Result.Complete || finished.Result.Returned != 3 || finished.Result.Pages[0] != reading.Result.Pages[0] {
		t.Fatalf("resume original checkpoint = %+v, %v", finished, err)
	}
}

func TestHotfixScanSourceRemovedBeforeCheckpointPublication(t *testing.T) {
	ctx := context.Background()
	root, pin, request := hotfixScanFixture(t)
	inspect := func(ctx context.Context, root, snapshot string, q HotfixRequest) (HotfixReading, error) {
		page, err := InspectHotfix(ctx, root, snapshot, q)
		if err != nil {
			return page, err
		}
		// The ordinary page now owns its source as well. Both must be
		// removed before the scan can encounter a missing-source gap.
		if _, err := removeScanCapture(t, root, page.Capture.ID); err != nil {
			t.Fatal(err)
		}
		if _, err := removeScanCapture(t, root, page.Result.Source.ID); err != nil {
			t.Fatal(err)
		}
		return page, nil
	}
	reading, err := scanHotfix(ctx, root, pin.ID, request, "", 1, inspect)
	if !errors.Is(err, vault.ErrMissingRecord) || reading.Capture.ID != "" || reading.Resume != "" {
		t.Fatalf("source removed before commit = %+v, %v; want missing source", reading, err)
	}
	for _, ref := range scanCaptureList(t, root) {
		if ref.Provenance.Kind == "hotfix-scan" {
			t.Fatal("checkpoint published after its source was removed")
		}
	}
}
