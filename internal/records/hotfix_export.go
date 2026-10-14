package records

import (
	"bytes"
	"context"
)

// ExportHotfixCSV preserves Hotfix's implicit overwrite policy and existing
// manifest schema. Publish the CSV and completion manifest under one target
// lease, shared with SQL CSV exports even when the callers use different homes.
func ExportHotfixCSV(ctx context.Context, root, output string, result RemoteHotfixResult) (CSVExportManifest, error) {
	if err := ctx.Err(); err != nil {
		return CSVExportManifest{}, err
	}
	var csv, sidecar bytes.Buffer
	manifest, err := EncodeHotfixCSV(&csv, result)
	if err != nil {
		return CSVExportManifest{}, err
	}
	if err := WriteCSVManifest(&sidecar, manifest); err != nil {
		return CSVExportManifest{}, err
	}
	dataDestination, err := openExportDestination(output, true)
	if err != nil {
		return CSVExportManifest{}, err
	}
	defer dataDestination.Close()
	manifestDestination, err := openExportDestination(output+".manifest.json", true)
	if err != nil {
		return CSVExportManifest{}, err
	}
	defer manifestDestination.Close()
	if err := outsideExportWorkspace(root, dataDestination, manifestDestination); err != nil {
		return CSVExportManifest{}, err
	}
	if err := publishCSVPair(ctx, dataDestination, manifestDestination, csv.Bytes(), sidecar.Bytes()); err != nil {
		return CSVExportManifest{}, err
	}
	return manifest, nil
}
