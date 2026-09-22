package vault

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"net/url"
	"os"
	"path/filepath"
	"strings"

	_ "modernc.org/sqlite"
)

var ErrGeneration = errors.New("vault.generation_conflict")
var ErrMissingRecord = errors.New("vault.record_missing")

// Metadata holds bounded documents; original reports and large data belong in
// blobs. Each mutation uses a short SQL transaction without user callbacks.
type Metadata struct{ db *sql.DB }

type Document struct {
	Key        string          `json:"key"`
	Generation int64           `json:"generation"`
	Value      json.RawMessage `json:"value"`
}

type Mutation struct {
	Key                string
	ExpectedGeneration int64 // Zero creates a new key; generations never reset.
	Value              json.RawMessage
}

func (s *Store) OpenMetadata(ctx context.Context) (*Metadata, error) {
	return s.openMetadata(ctx, false)
}

// ReadMetadata opens existing documents without initializing or migrating the
// schema. SQLite remains responsible for coordinating reads with live writers.
func (s *Store) ReadMetadata(ctx context.Context) (*Metadata, error) {
	return s.openMetadata(ctx, true)
}

func (s *Store) openMetadata(ctx context.Context, readOnly bool) (*Metadata, error) {
	if readOnly {
		if _, err := os.Stat(filepath.Join(s.root, "state", "toolkit.sqlite")); err != nil {
			if errors.Is(err, os.ErrNotExist) {
				return nil, ErrMissingRecord
			}
			return nil, err
		}
	} else {
		lease, err := AcquireLease(ctx, filepath.Join(s.root, "locks"), "metadata-schema")
		if err != nil {
			return nil, err
		}
		defer lease.Close()
	}
	path := filepath.ToSlash(filepath.Join(s.root, "state", "toolkit.sqlite"))
	if !strings.HasPrefix(path, "/") {
		path = "/" + path
	}
	u := url.URL{Scheme: "file", Path: path}
	q := url.Values{}
	if readOnly {
		q.Set("mode", "ro")
	}
	q.Add("_pragma", "busy_timeout(5000)")
	q.Add("_pragma", "synchronous(FULL)")
	u.RawQuery = q.Encode()
	db, err := sql.Open("sqlite", u.String())
	if err != nil {
		return nil, err
	}
	db.SetMaxOpenConns(1)
	accepted := false
	defer func() {
		if !accepted {
			db.Close()
		}
	}()
	var version int
	if err := db.QueryRowContext(ctx, "PRAGMA user_version").Scan(&version); err != nil {
		return nil, err
	}
	if version != 0 && version != 1 {
		return nil, ErrWorkspaceFormat
	}
	if readOnly && version != 1 {
		return nil, ErrWorkspaceFormat
	}
	if version == 0 {
		var tables int
		if err := db.QueryRowContext(ctx, "SELECT count(*) FROM sqlite_master WHERE type='table'").Scan(&tables); err != nil {
			return nil, err
		}
		if tables != 0 {
			return nil, ErrWorkspaceFormat
		}
		tx, err := db.BeginTx(ctx, nil)
		if err != nil {
			return nil, err
		}
		defer tx.Rollback()
		if _, err := tx.ExecContext(ctx, `CREATE TABLE documents (key TEXT PRIMARY KEY NOT NULL, generation INTEGER NOT NULL CHECK(generation>0), value BLOB NOT NULL CHECK(length(value)<=1048576)) WITHOUT ROWID; PRAGMA user_version=1;`); err != nil {
			return nil, err
		}
		if err := tx.Commit(); err != nil {
			return nil, err
		}
	}
	if !readOnly {
		if _, err := db.ExecContext(ctx, "PRAGMA journal_mode=WAL"); err != nil {
			return nil, err
		}
	}
	accepted = true
	return &Metadata{db: db}, nil
}

func (m *Metadata) Close() error { return m.db.Close() }

func (m *Metadata) ReadDocument(ctx context.Context, key string) (Document, error) {
	var doc Document
	doc.Key = key
	err := m.db.QueryRowContext(ctx, "SELECT generation,value FROM documents WHERE key=?", key).Scan(&doc.Generation, &doc.Value)
	if errors.Is(err, sql.ErrNoRows) {
		return Document{}, ErrMissingRecord
	}
	return doc, err
}

// CommitDocuments performs one atomic compare-and-swap across all supplied keys.
// A stale generation rolls back every change, including previously inserted keys.
func (m *Metadata) CommitDocuments(ctx context.Context, changes ...Mutation) error {
	if len(changes) == 0 || len(changes) > 64 {
		return errors.New("vault: invalid mutation count")
	}
	seen := map[string]bool{}
	for _, change := range changes {
		if change.Key == "" || len(change.Key) > 512 || seen[change.Key] || change.ExpectedGeneration < 0 || change.ExpectedGeneration == 1<<63-1 || len(change.Value) > 1<<20 || !json.Valid(change.Value) {
			return errors.New("vault: invalid mutation")
		}
		seen[change.Key] = true
	}
	tx, err := m.db.BeginTx(ctx, nil)
	if err != nil {
		return err
	}
	defer tx.Rollback()
	for _, change := range changes {
		var result sql.Result
		if change.ExpectedGeneration == 0 {
			result, err = tx.ExecContext(ctx, "INSERT INTO documents(key,generation,value) VALUES (?,1,?) ON CONFLICT(key) DO NOTHING", change.Key, []byte(change.Value))
		} else {
			result, err = tx.ExecContext(ctx, "UPDATE documents SET generation=generation+1,value=? WHERE key=? AND generation=?", []byte(change.Value), change.Key, change.ExpectedGeneration)
		}
		if err != nil {
			return err
		}
		count, err := result.RowsAffected()
		if err != nil {
			return err
		}
		if count != 1 {
			return fmt.Errorf("%w: %s", ErrGeneration, change.Key)
		}
	}
	return tx.Commit()
}

// DeleteDocuments removes documents in one atomic compare-and-swap. A stale
// generation rolls back every removal; documents are only ever deleted through
// an explicit caller decision, never by cache pruning.
type Deletion struct {
	Key                string
	ExpectedGeneration int64
}

func (m *Metadata) DeleteDocuments(ctx context.Context, removals ...Deletion) error {
	if len(removals) == 0 || len(removals) > 64 {
		return errors.New("vault: invalid deletion count")
	}
	seen := map[string]bool{}
	for _, removal := range removals {
		if removal.Key == "" || len(removal.Key) > 512 || seen[removal.Key] || removal.ExpectedGeneration <= 0 {
			return errors.New("vault: invalid deletion")
		}
		seen[removal.Key] = true
	}
	tx, err := m.db.BeginTx(ctx, nil)
	if err != nil {
		return err
	}
	defer tx.Rollback()
	for _, removal := range removals {
		result, err := tx.ExecContext(ctx, "DELETE FROM documents WHERE key=? AND generation=?", removal.Key, removal.ExpectedGeneration)
		if err != nil {
			return err
		}
		count, err := result.RowsAffected()
		if err != nil {
			return err
		}
		if count != 1 {
			return fmt.Errorf("%w: %s", ErrGeneration, removal.Key)
		}
	}
	return tx.Commit()
}

func (m *Metadata) ListDocuments(ctx context.Context, prefix, after string, limit int) ([]Document, error) {
	if prefix == "" || limit < 1 || limit > 1000 {
		return nil, errors.New("vault: invalid document page")
	}
	rows, err := m.db.QueryContext(ctx, "SELECT key,generation,value FROM documents WHERE substr(key,1,length(?))=? AND key>? ORDER BY key LIMIT ?", prefix, prefix, after, limit)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	result := make([]Document, 0)
	for rows.Next() {
		var doc Document
		if err := rows.Scan(&doc.Key, &doc.Generation, &doc.Value); err != nil {
			return nil, err
		}
		result = append(result, doc)
	}
	return result, rows.Err()
}
