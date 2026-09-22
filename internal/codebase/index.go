package codebase

import (
	"bytes"
	"context"
	"crypto/sha256"
	"database/sql"
	"encoding/hex"
	"encoding/json"
	"errors"
	"net/url"
	"os"
	"path"
	"path/filepath"
	"strings"

	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
	_ "modernc.org/sqlite"
)

const indexSchema = "lycheedev.source-index.v2"

// indexSchemaV1 stays readable so a published index from the previous schema
// is never silently unusable. Asset rows require the current schema and report
// a rebuild hint instead of pretending an asset search was empty.
const indexSchemaV1 = "lycheedev.source-index.v1"

type IndexSummary struct {
	Schema           string           `json:"schema"`
	Repository       string           `json:"repository"`
	Product          string           `json:"product"`
	Commit           string           `json:"commit"`
	Parser           string           `json:"parser"`
	Documents        int              `json:"documents"`
	Declarations     int              `json:"declarations"`
	Relationships    int              `json:"relationships"`
	Assets           int              `json:"assets"`
	Diagnostics      int              `json:"diagnostics"`
	Complete         bool             `json:"complete"`
	DiagnosticSample []FileDiagnostic `json:"diagnosticSample,omitempty"`
}

type FileDiagnostic struct {
	Path    string `json:"path"`
	Line    int    `json:"line"`
	Message string `json:"message"`
}

func (b *Browser) indexPathFor(schema string, pin selection.SourcePin) string {
	digest := sha256.Sum256([]byte(schema + "\x00" + pin.Repository + "\x00" + pin.Product + "\x00" + pin.ExactCommit + "\x00" + pin.ParserRevision))
	return filepath.Join(b.store.Root(), "indexes", "source-"+hex.EncodeToString(digest[:])+".sqlite")
}

func (b *Browser) indexPath(pin selection.SourcePin) string {
	return b.indexPathFor(indexSchema, pin)
}

// IndexSource builds a private database and publishes it only after the whole
// pinned tree is processed. Existing readers never observe a partial rebuild.
// Syntax failures are retained as diagnostics and make Complete false.
func (b *Browser) IndexSource(ctx context.Context, pin selection.SourcePin) (IndexSummary, error) {
	files, err := b.sourceTree(ctx, pin)
	if err != nil {
		return IndexSummary{}, err
	}
	enumerate := func(list []treeFile, visit func(treeFile, []byte) error) error {
		return b.visitDocuments(ctx, pin, list, visit)
	}
	return b.buildIndex(ctx, pin, files, enumerate)
}

// IndexFixture indexes a local fixture directory under the deterministic
// synthetic commit of its absolute path. It is the offline fixture counterpart
// of IndexSource and publishes through the same atomic pipeline.
func (b *Browser) IndexFixture(ctx context.Context, pin selection.SourcePin, root string) (IndexSummary, error) {
	if pin.Repository == "" || pin.Product == "" || !objectID(pin.ExactCommit) || pin.ParserRevision != ParserRevision {
		return IndexSummary{}, errors.New("codebase.invalid_source_pin")
	}
	files, err := directoryTree(root)
	if err != nil {
		return IndexSummary{}, err
	}
	enumerate := func(list []treeFile, visit func(treeFile, []byte) error) error {
		return visitDirectory(ctx, root, list, visit)
	}
	return b.buildIndex(ctx, pin, files, enumerate)
}

func (b *Browser) buildIndex(ctx context.Context, pin selection.SourcePin, files []treeFile, enumerate func([]treeFile, func(treeFile, []byte) error) error) (IndexSummary, error) {
	var summary IndexSummary
	lease, err := vault.AcquireLease(ctx, filepath.Join(b.store.Root(), "locks"), "index:"+b.indexPath(pin))
	if err != nil {
		return summary, err
	}
	defer lease.Close()
	if _, err := os.Lstat(b.indexPath(pin)); err == nil {
		db, s, err := b.openIndex(ctx, pin)
		if db != nil {
			db.Close()
		}
		return s, err
	} else if !errors.Is(err, os.ErrNotExist) {
		return summary, err
	}
	stage, err := os.CreateTemp(filepath.Join(b.store.Root(), "tmp"), "source-index-*.sqlite")
	if err != nil {
		return summary, err
	}
	stagePath := stage.Name()
	stage.Close()
	defer os.Remove(stagePath)
	defer os.Remove(stagePath + "-journal")
	db, err := sql.Open("sqlite", stagePath)
	if err != nil {
		return summary, err
	}
	defer db.Close()
	db.SetMaxOpenConns(1)
	if _, err := db.ExecContext(ctx, `PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL;
CREATE TABLE manifest(summary TEXT NOT NULL);
CREATE TABLE documents(path TEXT PRIMARY KEY, sha256 TEXT NOT NULL, bytes INTEGER NOT NULL);
CREATE TABLE entries(kind TEXT NOT NULL, name TEXT NOT NULL, target TEXT NOT NULL, category TEXT NOT NULL, confidence TEXT NOT NULL, path TEXT NOT NULL, line INTEGER NOT NULL, end_line INTEGER NOT NULL, signature TEXT NOT NULL);
CREATE TABLE diagnostics(path TEXT NOT NULL, line INTEGER NOT NULL, message TEXT NOT NULL);
CREATE TABLE assets(path TEXT PRIMARY KEY, normalized_path TEXT NOT NULL, sha256 TEXT NOT NULL, bytes INTEGER NOT NULL, extension TEXT NOT NULL, mime TEXT NOT NULL, format TEXT NOT NULL, width INTEGER NOT NULL, height INTEGER NOT NULL);
CREATE INDEX entry_name ON entries(name);
CREATE INDEX entry_target ON entries(target);
CREATE INDEX entry_path ON entries(path,line);
CREATE INDEX asset_norm ON assets(normalized_path);`); err != nil {
		return summary, err
	}
	tx, err := db.BeginTx(ctx, nil)
	if err != nil {
		return summary, err
	}
	defer tx.Rollback()
	row, err := tx.PrepareContext(ctx, "INSERT INTO entries VALUES(?,?,?,?,?,?,?,?,?)")
	if err != nil {
		return summary, err
	}
	defer row.Close()
	summary = IndexSummary{Schema: indexSchema, Repository: pin.Repository, Product: pin.Product, Commit: pin.ExactCommit, Parser: pin.ParserRevision, Complete: true}
	recordAsset := func(file treeFile, data []byte) error {
		extension := strings.ToLower(path.Ext(file.path))
		width, height, format := 0, 0, strings.TrimPrefix(extension, ".")
		digest := ""
		if data != nil {
			width, height, format = assetImageInfo(data, extension)
			sum := sha256.Sum256(data)
			digest = hex.EncodeToString(sum[:])
			if _, err := b.store.PublishBlob(ctx, vault.BlobInput{Reader: bytes.NewReader(data), MaxBytes: maxAssetBytes, ExpectedSHA256: digest}); err != nil {
				return err
			}
		}
		if _, err := tx.ExecContext(ctx, "INSERT INTO assets VALUES(?,?,?,?,?,?,?,?,?)",
			file.path, normalizeAssetPath(file.path), digest, file.size, extension, assetMIME(extension), format, width, height); err != nil {
			return err
		}
		summary.Assets++
		return nil
	}
	err = enumerate(files, func(file treeFile, data []byte) error {
		if file.asset {
			return recordAsset(file, data)
		}
		facts, err := AnalyzeDocument(ctx, file.path, data)
		if err != nil {
			return err
		}
		blob, err := b.store.PublishBlob(ctx, vault.BlobInput{Reader: bytes.NewReader(data), MaxBytes: maxSourceBytes})
		if err != nil {
			return err
		}
		if _, err := tx.ExecContext(ctx, "INSERT INTO documents VALUES(?,?,?)", file.path, blob.SHA256, blob.Bytes); err != nil {
			return err
		}
		for _, d := range facts.Declarations {
			if _, err := row.ExecContext(ctx, "declaration", d.Name, "", d.Category, "exact", file.path, d.Line, d.EndLine, d.Signature); err != nil {
				return err
			}
		}
		for _, r := range facts.Relationships {
			if _, err := row.ExecContext(ctx, "relationship", r.From, r.To, r.Category, r.Confidence, file.path, r.Line, r.Line, ""); err != nil {
				return err
			}
		}
		for _, load := range facts.Loads {
			if _, err := row.ExecContext(ctx, "load", file.path, load.Path, "file", "exact", file.path, load.Line, load.Line, ""); err != nil {
				return err
			}
		}
		for _, header := range facts.Headers {
			if _, err := row.ExecContext(ctx, "header", header.Key, header.Value, "toc-field", "exact", file.path, header.Line, header.Line, ""); err != nil {
				return err
			}
		}
		for _, note := range facts.Diagnostics {
			if _, err := tx.ExecContext(ctx, "INSERT INTO diagnostics VALUES(?,?,?)", file.path, note.Line, note.Message); err != nil {
				return err
			}
			if len(summary.DiagnosticSample) < 20 {
				summary.DiagnosticSample = append(summary.DiagnosticSample, FileDiagnostic{Path: file.path, Line: note.Line, Message: note.Message})
			}
		}
		summary.Documents++
		summary.Declarations += len(facts.Declarations)
		summary.Relationships += len(facts.Relationships)
		summary.Diagnostics += len(facts.Diagnostics)
		return nil
	})
	if err != nil {
		return IndexSummary{}, err
	}
	// Assets above the read budget keep metadata-only rows from listing facts.
	for _, file := range files {
		if file.asset && !treeNeedsRead(file) {
			if err := recordAsset(file, nil); err != nil {
				return IndexSummary{}, err
			}
		}
	}
	summary.Complete = summary.Diagnostics == 0
	raw, err := json.Marshal(summary)
	if err != nil {
		return IndexSummary{}, err
	}
	if _, err := tx.ExecContext(ctx, "INSERT INTO manifest VALUES(?)", string(raw)); err != nil {
		return IndexSummary{}, err
	}
	if err := tx.Commit(); err != nil {
		return IndexSummary{}, err
	}
	if err := row.Close(); err != nil {
		return IndexSummary{}, err
	}
	if err := db.Close(); err != nil {
		return IndexSummary{}, err
	}
	if err := ctx.Err(); err != nil {
		return IndexSummary{}, err
	}
	if err := os.Rename(stagePath, b.indexPath(pin)); err != nil {
		return IndexSummary{}, err
	}
	return summary, nil
}

// errIndexNotReady reports a missing published index for the requested pin.
var errIndexNotReady = errors.New("codebase.index_not_ready: run source index with the same snapshot")

func (b *Browser) openIndex(ctx context.Context, pin selection.SourcePin) (*sql.DB, IndexSummary, error) {
	var summary IndexSummary
	for _, schema := range []string{indexSchema, indexSchemaV1} {
		name := b.indexPathFor(schema, pin)
		if _, err := os.Stat(name); err != nil {
			continue
		}
		uriPath := filepath.ToSlash(name)
		if !strings.HasPrefix(uriPath, "/") {
			uriPath = "/" + uriPath
		}
		uri := (&url.URL{Scheme: "file", Path: uriPath, RawQuery: "mode=ro"}).String()
		db, err := sql.Open("sqlite", uri)
		if err != nil {
			return nil, summary, err
		}
		db.SetMaxOpenConns(1)
		var raw string
		err = db.QueryRowContext(ctx, "SELECT summary FROM manifest").Scan(&raw)
		if err == nil {
			err = json.Unmarshal([]byte(raw), &summary)
		}
		if err == nil && (summary.Schema != schema || summary.Commit != pin.ExactCommit || summary.Repository != pin.Repository || summary.Product != pin.Product || summary.Parser != pin.ParserRevision) {
			err = errors.New("codebase.index_identity_mismatch")
		}
		if err != nil {
			db.Close()
			return nil, summary, err
		}
		// Older cache entries may lack the bounded preview; diagnostics themselves
		// have always been stored independently of the completion manifest.
		if summary.Diagnostics > 0 && len(summary.DiagnosticSample) == 0 {
			rows, err := db.QueryContext(ctx, "SELECT path,line,message FROM diagnostics ORDER BY path,line LIMIT 20")
			if err != nil {
				db.Close()
				return nil, summary, err
			}
			for rows.Next() {
				var note FileDiagnostic
				if err := rows.Scan(&note.Path, &note.Line, &note.Message); err != nil {
					rows.Close()
					db.Close()
					return nil, summary, err
				}
				summary.DiagnosticSample = append(summary.DiagnosticSample, note)
			}
			err = rows.Err()
			rows.Close()
			if err != nil {
				db.Close()
				return nil, summary, err
			}
		}
		return db, summary, nil
	}
	return nil, summary, errIndexNotReady
}

// requireAssetIndex refuses asset lookups on a legacy-schema index instead of
// reporting an empty, misleading result.
func requireAssetIndex(summary IndexSummary) error {
	if summary.Schema != indexSchema {
		return errors.New("codebase.index_rebuild_required: asset rows need the current index schema")
	}
	return nil
}

type SymbolMatch struct {
	Kind       string `json:"kind"`
	Name       string `json:"name"`
	Target     string `json:"target,omitempty"`
	Category   string `json:"category"`
	Confidence string `json:"confidence"`
	Path       string `json:"path"`
	Line       int    `json:"line"`
	EndLine    int    `json:"endLine"`
	Signature  string `json:"signature,omitempty"`
}
type SymbolMatches struct {
	Index     IndexSummary  `json:"index"`
	Matches   []SymbolMatch `json:"matches"`
	Truncated bool          `json:"truncated"`
}

func (b *Browser) FindSymbols(ctx context.Context, pin selection.SourcePin, term string, limit int) (SymbolMatches, error) {
	result := SymbolMatches{Matches: []SymbolMatch{}}
	if term == "" || len(term) > 512 || limit < 1 || limit > 200 {
		return result, errors.New("codebase.invalid_symbol_query")
	}
	db, summary, err := b.openIndex(ctx, pin)
	if err != nil {
		return result, err
	}
	defer db.Close()
	result.Index = summary
	rows, err := db.QueryContext(ctx, `SELECT kind,name,target,category,confidence,path,line,end_line,signature FROM entries WHERE name=? OR target=? ORDER BY kind,path,line,name,target,category LIMIT ?`, term, term, limit+1)
	if err != nil {
		return result, err
	}
	defer rows.Close()
	for rows.Next() {
		var match SymbolMatch
		if err := rows.Scan(&match.Kind, &match.Name, &match.Target, &match.Category, &match.Confidence, &match.Path, &match.Line, &match.EndLine, &match.Signature); err != nil {
			return result, err
		}
		if len(result.Matches) == limit {
			result.Truncated = true
			break
		}
		result.Matches = append(result.Matches, match)
	}
	return result, rows.Err()
}
