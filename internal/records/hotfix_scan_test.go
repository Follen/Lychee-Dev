package records

import (
	"context"
	"encoding/binary"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
)

func hotfixScanFixture(t *testing.T) (string, selection.PinnedSet, HotfixRequest) {
	t.Helper()
	ctx := context.Background()
	root := filepath.Join(t.TempDir(), "workspace")
	if _, err := vault.Initialize(ctx, root); err != nil {
		t.Fatal(err)
	}
	pin, err := vault.WriteMetadata(ctx, root, func(_ *vault.Store, m *vault.Metadata) (selection.PinnedSet, error) {
		return selection.OpenPinner(m).PinSelection(ctx, selection.SelectionSpec{Data: &selection.DataPin{Product: "retail", Region: "cn", Language: "zhCN", FullBuild: "12.1.0.69875", BuildConfig: strings.Repeat("a", 32), CDNConfig: strings.Repeat("b", 32), DefinitionCommit: strings.Repeat("c", 40)}})
	})
	if err != nil {
		t.Fatal(err)
	}
	raw := make([]byte, 44)
	copy(raw, "XFTH")
	binary.LittleEndian.PutUint32(raw[4:], 9)
	binary.LittleEndian.PutUint32(raw[8:], 69875)
	for i := 0; i < 3; i++ {
		h := make([]byte, 32)
		copy(h, "XFTH")
		binary.LittleEndian.PutUint32(h[16:], 0xabcdef01)
		binary.LittleEndian.PutUint32(h[20:], uint32(i+1))
		h[28] = 2
		raw = append(raw, h...)
	}
	file := filepath.Join(t.TempDir(), "DBCache.bin")
	if err := os.WriteFile(file, raw, 0600); err != nil {
		t.Fatal(err)
	}
	request := HotfixRequest{File: file, Filter: CacheFilter{Limit: 1}, MaxBytes: 1 << 20}
	return root, pin, request
}

func TestHotfixScanCancellationReturnsSavedPage(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	root, pin, request := hotfixScanFixture(t)
	calls := 0
	inspect := func(ctx context.Context, root, pin string, q HotfixRequest) (HotfixReading, error) {
		calls++
		if calls == 2 {
			cancel()
		}
		return InspectHotfix(ctx, root, pin, q)
	}
	reading, err := scanHotfix(ctx, root, pin.ID, request, "", 3, inspect)
	if !errors.Is(err, context.Canceled) || reading.Resume == "" || reading.Result.Returned != 1 || len(reading.Result.Pages) != 1 || reading.Result.Complete {
		t.Fatalf("checkpoint=%+v err=%v", reading, err)
	}
	request.File, request.From = "", reading.Result.Source.ID
	finished, err := ScanHotfix(context.Background(), root, reading.Result.Snapshot, request, reading.Resume, 3)
	if err != nil || !finished.Result.Complete || finished.Result.Returned != 3 || len(finished.Result.Pages) != 3 {
		t.Fatalf("resume=%+v err=%v", finished, err)
	}
}

func TestHotfixScanPendingProtectsSourceAndResumes(t *testing.T) {
	ctx := context.Background()
	root, pin, request := hotfixScanFixture(t)
	reading, err := ScanHotfix(ctx, root, pin.ID, request, "", 1)
	if err != nil || reading.Resume == "" || reading.Result.Returned != 1 || len(reading.Result.Pages) != 1 || reading.Result.Complete {
		t.Fatalf("pending checkpoint = %+v, %v", reading, err)
	}
	// The persisted checkpoint must protect its immutable input even after the
	// original caller has stopped. A rejected removal must not break resume.
	_, removeErr := vault.WriteMetadata(context.Background(), root, func(s *vault.Store, m *vault.Metadata) (evidence.RemoveResult, error) {
		return evidence.OpenArchive(s, m).RemoveCapture(context.Background(), reading.Result.Source.ID)
	})
	if !errors.Is(removeErr, evidence.ErrCaptureReferenced) {
		t.Fatalf("pending source removal = %v, want capture referenced", removeErr)
	}
	if reading.Capture.Provenance.Locator != reading.Result.Source.ID {
		t.Fatalf("checkpoint locator = %q, want source ID", reading.Capture.Provenance.Locator)
	}
	request.File, request.From = "", reading.Result.Source.ID
	finished, err := ScanHotfix(ctx, root, reading.Result.Snapshot, request, reading.Resume, 3)
	if err != nil || !finished.Result.Complete || finished.Result.Returned != 3 || len(finished.Result.Pages) != 3 {
		t.Fatalf("resume after refused deletion = %+v, %v", finished, err)
	}
}
