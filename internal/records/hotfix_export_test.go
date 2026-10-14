package records

import (
	"bytes"
	"context"
	"errors"
	"os"
	"path/filepath"
	"testing"
	"time"
)

func TestHotfixCSVUsesExistingSQLDestinationLease(t *testing.T) {
	output := filepath.Join(t.TempDir(), "shared.csv")
	original, sidecar := []byte("original CSV"), []byte("original manifest")
	if err := os.WriteFile(output, original, 0600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(output+".manifest.json", sidecar, 0600); err != nil {
		t.Fatal(err)
	}
	destination, err := openExportDestination(output, true)
	if err != nil {
		t.Fatal(err)
	}
	defer destination.Close()
	lease, err := acquireExportLease(context.Background(), destination.path, "sql-csv")
	if err != nil {
		t.Fatal(err)
	}
	defer lease.Close()
	result := RemoteHotfixResult{Complete: true, Records: []HotfixRecord{{ID: 17, TableName: "SpellName", Build: "70000"}}}
	ctx, cancel := context.WithTimeout(context.Background(), 100*time.Millisecond)
	_, err = ExportHotfixCSV(ctx, t.TempDir(), output, result)
	cancel()
	if !errors.Is(err, context.DeadlineExceeded) {
		t.Fatalf("Hotfix did not share SQL's destination lease: %v", err)
	}
	for path, want := range map[string][]byte{output: original, output + ".manifest.json": sidecar} {
		if raw, err := os.ReadFile(path); err != nil || !bytes.Equal(raw, want) {
			t.Fatalf("cancelled waiter changed %s: %v", path, err)
		}
	}
	if err := lease.Close(); err != nil {
		t.Fatal(err)
	}
	// Hotfix retains implicit overwrite; no new --overwrite flag is required.
	manifest, err := ExportHotfixCSV(context.Background(), t.TempDir(), output, result)
	if err != nil {
		t.Fatal(err)
	}
	var encoded, encodedManifest bytes.Buffer
	want, err := EncodeHotfixCSV(&encoded, result)
	if err != nil {
		t.Fatal(err)
	}
	if err := WriteCSVManifest(&encodedManifest, want); err != nil {
		t.Fatal(err)
	}
	if manifest.Content != want.Content || manifest.RowCount != want.RowCount {
		t.Fatal("manifest schema changed")
	}
	for path, want := range map[string][]byte{output: encoded.Bytes(), output + ".manifest.json": encodedManifest.Bytes()} {
		if raw, err := os.ReadFile(path); err != nil || !bytes.Equal(raw, want) {
			t.Fatalf("published bytes changed for %s: %v", path, err)
		}
	}
}

func TestHotfixCSVPreflightAndCancellationPreserveExistingFiles(t *testing.T) {
	for _, scenario := range []string{"cancelled", "encoding", "manifest-directory"} {
		t.Run(scenario, func(t *testing.T) {
			output := filepath.Join(t.TempDir(), "hotfix.csv")
			original := []byte("caller CSV")
			if err := os.WriteFile(output, original, 0600); err != nil {
				t.Fatal(err)
			}
			result := RemoteHotfixResult{Complete: true, Records: []HotfixRecord{{ID: 1, TableName: "SpellName"}}}
			ctx := context.Background()
			var want error
			switch scenario {
			case "cancelled":
				var cancel context.CancelFunc
				ctx, cancel = context.WithCancel(ctx)
				cancel()
				want = context.Canceled
			case "encoding":
				result.Records[0].TableName = string([]byte{255})
				want = ErrExportEncoding
			case "manifest-directory":
				if err := os.Mkdir(output+".manifest.json", 0700); err != nil {
					t.Fatal(err)
				}
				want = ErrExportConflict
			}
			if _, err := ExportHotfixCSV(ctx, t.TempDir(), output, result); !errors.Is(err, want) {
				t.Fatalf("export error=%v, want %v", err, want)
			}
			if raw, err := os.ReadFile(output); err != nil || !bytes.Equal(raw, original) {
				t.Fatalf("failed preflight/cancellation replaced CSV: %v", err)
			}
			entries, err := os.ReadDir(filepath.Dir(output))
			if err != nil {
				t.Fatal(err)
			}
			for _, entry := range entries {
				if len(entry.Name()) >= 18 && entry.Name()[:18] == ".lycheedev-export-" {
					t.Fatalf("stage leaked: %s", entry.Name())
				}
			}
		})
	}
}
