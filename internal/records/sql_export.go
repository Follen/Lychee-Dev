package records

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/csv"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"path/filepath"
	"strings"

	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/vault"
)

// QueryCSVManifest is the completion record for a static SQL CSV export.
// It points to both the archived query result and the archived exact CSV bytes.
type QueryCSVManifest struct {
	Schema       string   `json:"schema"`
	Snapshot     string   `json:"snapshot"`
	Output       string   `json:"output"`
	SHA256       string   `json:"sha256"`
	Bytes        int      `json:"bytes"`
	Columns      []string `json:"columns"`
	Rows         int      `json:"rows"`
	QueryCapture string   `json:"queryCapture"`
	CSVCapture   string   `json:"csvCapture"`
	Complete     bool     `json:"complete"`
}

type QueryCSVExport struct {
	Manifest QueryCSVManifest
	Capture  evidence.CaptureRef
}

var ErrQueryCSVLimit = errors.New("records.query_csv_limit")

// ExportQueryCSV publishes complete CSV bytes first and the manifest last.
// Both destinations must be outside the managed workspace; existing files are
// refused unless replacement is explicit. No result is claimed before the
// completion manifest has been published.
func ExportQueryCSV(ctx context.Context, root, snapshot string, reading QueryReading, output string, overwrite bool) (QueryCSVExport, error) {
	if snapshot == "" || reading.Capture.ID == "" {
		return QueryCSVExport{}, ErrExportPath
	}
	dataDestination, err := openExportDestination(output, overwrite)
	if err != nil {
		return QueryCSVExport{}, err
	}
	defer dataDestination.Close()
	manifestDestination, err := openExportDestination(output+".manifest.json", overwrite)
	if err != nil {
		return QueryCSVExport{}, err
	}
	defer manifestDestination.Close()
	workspace, err := filepath.EvalSymlinks(root)
	if err != nil {
		return QueryCSVExport{}, err
	}
	workspace, err = filepath.Abs(workspace)
	if err != nil {
		return QueryCSVExport{}, err
	}
	for _, destination := range []*exportDestination{dataDestination, manifestDestination} {
		if relative, err := filepath.Rel(workspace, destination.path); err == nil && relative != ".." && !strings.HasPrefix(relative, ".."+string(filepath.Separator)) {
			return QueryCSVExport{}, fmt.Errorf("%w: output must be outside the managed workspace", ErrExportPath)
		}
	}
	var buffer bytes.Buffer
	writer := csv.NewWriter(&buffer)
	columns := reading.Result.Result.Columns
	if err := writer.Write(columns); err != nil {
		return QueryCSVExport{}, err
	}
	for _, row := range reading.Result.Result.Rows {
		if err := ctx.Err(); err != nil {
			return QueryCSVExport{}, err
		}
		if len(row) != len(columns) {
			return QueryCSVExport{}, errors.New("records.sql_csv_row_width")
		}
		cells := make([]string, len(row))
		for i, value := range row {
			cell, err := queryCSVCell(value)
			if err != nil {
				return QueryCSVExport{}, err
			}
			cells[i] = cell
		}
		if err := writer.Write(cells); err != nil {
			return QueryCSVExport{}, err
		}
		if buffer.Len() > 32<<20 {
			return QueryCSVExport{}, ErrQueryCSVLimit
		}
	}
	writer.Flush()
	if err := writer.Error(); err != nil {
		return QueryCSVExport{}, err
	}
	if buffer.Len() > 32<<20 {
		return QueryCSVExport{}, ErrQueryCSVLimit
	}
	raw := buffer.Bytes()
	digest := sha256.Sum256(raw)
	var capture evidence.CaptureRef
	_, err = vault.WriteMetadata(ctx, root, func(s *vault.Store, m *vault.Metadata) (bool, error) {
		var commitErr error
		capture, commitErr = evidence.OpenArchive(s, m).CommitCapture(ctx, evidence.CaptureDraft{
			Reader: bytes.NewReader(raw), MaxBytes: 32 << 20, MediaType: "text/csv; charset=utf-8", Complete: true,
			Provenance: evidence.Provenance{Kind: "data-query-csv", Locator: reading.Capture.ID, Snapshot: snapshot},
		})
		return commitErr == nil, commitErr
	})
	if err != nil {
		return QueryCSVExport{}, err
	}
	manifest := QueryCSVManifest{Schema: "lycheedev.query-csv.v1", Snapshot: snapshot, Output: dataDestination.path,
		SHA256: hex.EncodeToString(digest[:]), Bytes: len(raw), Columns: append([]string(nil), columns...), Rows: len(reading.Result.Result.Rows),
		QueryCapture: reading.Capture.ID, CSVCapture: capture.ID, Complete: true}
	manifestRaw, err := json.MarshalIndent(manifest, "", "  ")
	if err != nil {
		return QueryCSVExport{}, err
	}
	if err := dataDestination.publish(ctx, raw); err != nil {
		return QueryCSVExport{}, err
	}
	if err := manifestDestination.publish(ctx, append(manifestRaw, '\n')); err != nil {
		return QueryCSVExport{}, err
	}
	return QueryCSVExport{Manifest: manifest, Capture: capture}, nil
}

func queryCSVCell(value any) (string, error) {
	switch typed := value.(type) {
	case nil:
		return "", nil
	case string:
		return typed, nil
	case bool:
		if typed {
			return "true", nil
		}
		return "false", nil
	case json.Number:
		return typed.String(), nil
	case float64, float32, int, int32, int64, uint, uint32, uint64:
		return fmt.Sprint(typed), nil
	default:
		raw, err := json.Marshal(typed)
		return string(raw), err
	}
}
