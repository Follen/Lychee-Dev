// SPDX-License-Identifier: MIT
package records

import (
	"crypto/sha256"
	"encoding/csv"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"time"
	"unicode/utf8"

	"github.com/follenfang/lycheedev/internal/vault"
)

var ErrExportEncoding = errors.New("records.export_encoding")

// CSVExportSchema versions the machine-readable manifest that accompanies every
// CSV export encoding.
const CSVExportSchema = "lycheedev.csv-export.v1"

// CSVTable is a rectangular text table with caller-stable column ordering.
type CSVTable struct {
	Columns []string   `json:"columns"`
	Rows    [][]string `json:"rows"`
}

// CSVSourceIdentity pins the exact source an export was encoded from.
type CSVSourceIdentity struct {
	Provider  string    `json:"provider"`
	Product   string    `json:"product"`
	FullBuild string    `json:"fullBuild"`
	Region    string    `json:"region"`
	Locale    string    `json:"locale,omitempty"`
	Ref       string    `json:"ref"`
	FetchedAt time.Time `json:"fetchedAt"`
}

// CSVExportManifest records schema, columns, row count, the fixed source
// identity and truncation/completeness so a consumer can verify the CSV bytes
// without trusting the transport.
type CSVExportManifest struct {
	Schema           string            `json:"schema"`
	Columns          []string          `json:"columns"`
	RowCount         int               `json:"rowCount"`
	Source           CSVSourceIdentity `json:"source"`
	Content          vault.BlobRef     `json:"content"`
	Complete         bool              `json:"complete"`
	Truncated        bool              `json:"truncated"`
	TruncationReason string            `json:"truncationReason,omitempty"`
}

// CSVExportRequest assembles one encoding run.
type CSVExportRequest struct {
	Table            CSVTable
	Source           CSVSourceIdentity
	Complete         bool
	Truncated        bool
	TruncationReason string
}

// EncodeCSV writes RFC 4180 CSV (comma separated, CRLF line endings and CRLF
// normalized embedded newlines, standard quoting) and returns its
// machine-readable manifest. The manifest's content digest describes the exact
// CSV bytes written.
func EncodeCSV(dst io.Writer, request CSVExportRequest) (CSVExportManifest, error) {
	if request.Complete && request.Truncated {
		return CSVExportManifest{}, fmt.Errorf("%w: a result cannot be complete and truncated", ErrExportEncoding)
	}
	if request.Truncated && request.TruncationReason == "" {
		return CSVExportManifest{}, fmt.Errorf("%w: truncated exports need a truncation reason", ErrExportEncoding)
	}
	columns := request.Table.Columns
	if len(columns) == 0 {
		return CSVExportManifest{}, fmt.Errorf("%w: no columns", ErrExportEncoding)
	}
	seen := map[string]bool{}
	for _, column := range columns {
		if column == "" || !utf8.ValidString(column) || seen[column] {
			return CSVExportManifest{}, fmt.Errorf("%w: invalid or duplicate column %q", ErrExportEncoding, column)
		}
		seen[column] = true
	}
	if len(request.Table.Rows) > 1_000_000 {
		return CSVExportManifest{}, fmt.Errorf("%w: row limit exceeded", ErrExportEncoding)
	}
	counter := &countingWriter{dst: dst}
	hash := sha256.New()
	writer := csv.NewWriter(io.MultiWriter(hash, counter))
	writer.UseCRLF = true
	if err := writer.Write(columns); err != nil {
		return CSVExportManifest{}, fmt.Errorf("%w: %v", ErrExportEncoding, err)
	}
	for index, row := range request.Table.Rows {
		if len(row) != len(columns) {
			return CSVExportManifest{}, fmt.Errorf("%w: row %d has %d cells for %d columns", ErrExportEncoding, index, len(row), len(columns))
		}
		for _, cell := range row {
			if !utf8.ValidString(cell) {
				return CSVExportManifest{}, fmt.Errorf("%w: row %d carries invalid UTF-8", ErrExportEncoding, index)
			}
		}
		if err := writer.Write(row); err != nil {
			return CSVExportManifest{}, fmt.Errorf("%w: %v", ErrExportEncoding, err)
		}
	}
	writer.Flush()
	if err := writer.Error(); err != nil {
		return CSVExportManifest{}, fmt.Errorf("%w: %v", ErrExportEncoding, err)
	}
	manifest := CSVExportManifest{
		Schema:           CSVExportSchema,
		Columns:          append([]string(nil), columns...),
		RowCount:         len(request.Table.Rows),
		Source:           request.Source,
		Content:          vault.BlobRef{SHA256: hex.EncodeToString(hash.Sum(nil)), Bytes: counter.written},
		Complete:         request.Complete,
		Truncated:        request.Truncated,
		TruncationReason: request.TruncationReason,
	}
	return manifest, nil
}

// WriteCSVManifest writes the machine-readable manifest next to an export.
func WriteCSVManifest(dst io.Writer, manifest CSVExportManifest) error {
	raw, err := json.MarshalIndent(manifest, "", "  ")
	if err != nil {
		return err
	}
	_, err = dst.Write(append(raw, '\n'))
	return err
}

type countingWriter struct {
	dst     io.Writer
	written int64
}

func (w *countingWriter) Write(p []byte) (int, error) {
	n, err := w.dst.Write(p)
	w.written += int64(n)
	return n, err
}

// HotfixCSVColumns is the stable hotfix CSV column order.
var HotfixCSVColumns = []string{"id", "push_id", "record_id", "table_name", "status", "build", "region_id", "locale", "payload_length"}

// HotfixCSVTable renders hotfix records in the stable column order. Optional
// numerics absent from a source render as empty cells instead of a fabricated
// zero; payload bytes stay out of the table and are reported as their length.
func HotfixCSVTable(records []HotfixRecord) CSVTable {
	rows := make([][]string, 0, len(records))
	for _, record := range records {
		id := ""
		if record.ID != 0 {
			id = fmt.Sprint(record.ID)
		}
		region := ""
		if record.RegionID != nil {
			region = fmt.Sprint(*record.RegionID)
		}
		rows = append(rows, []string{
			id,
			fmt.Sprint(record.Push),
			fmt.Sprint(record.RecordID),
			record.TableName,
			fmt.Sprint(record.Status),
			record.Build,
			region,
			record.Locale,
			fmt.Sprint(record.PayloadLength),
		})
	}
	return CSVTable{Columns: append([]string(nil), HotfixCSVColumns...), Rows: rows}
}

// HotfixCSVRequest maps one result onto an export request with its fixed source
// identity and completeness.
func HotfixCSVRequest(result RemoteHotfixResult) CSVExportRequest {
	locale := result.Query.Locale
	reason := result.Coverage.TruncationReason
	if reason == "" && result.Truncated {
		reason = "limit"
	}
	return CSVExportRequest{
		Table: HotfixCSVTable(result.Records),
		Source: CSVSourceIdentity{
			Provider:  result.Change.Provider,
			Product:   result.Query.Product,
			FullBuild: result.Query.FullBuild,
			Region:    result.Query.Region,
			Locale:    locale,
			Ref:       result.Change.SHA256,
			FetchedAt: result.Change.FetchedAt,
		},
		Complete:         result.Complete,
		Truncated:        result.Truncated,
		TruncationReason: reason,
	}
}

// EncodeHotfixCSV encodes one hotfix result as CSV with its manifest.
func EncodeHotfixCSV(dst io.Writer, result RemoteHotfixResult) (CSVExportManifest, error) {
	return EncodeCSV(dst, HotfixCSVRequest(result))
}
