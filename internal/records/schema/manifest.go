package schema

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"io"
	"regexp"
	"strconv"
	"strings"
	"unicode/utf8"
)

// TableIdentity names a definition and its distinct legacy/current file IDs.
// A manifest is source metadata, not proof that a file exists in a given build.
type TableIdentity struct {
	Name          string `json:"name"`
	Hash          uint32 `json:"hash"`
	DB2FileDataID uint32 `json:"db2FileDataId"`
	DBCFileDataID uint32 `json:"dbcFileDataId"`
}

type Manifest struct {
	digest string
	tables map[string]TableIdentity
}

var tableNamePattern = regexp.MustCompile(`^[A-Za-z_][A-Za-z0-9_-]{0,127}$`)

// ParseManifest rejects ambiguous names and file IDs rather than choosing the
// last JSON entry. Hash collisions remain legal: a hash alone is not identity.
func ParseManifest(ctx context.Context, raw []byte) (*Manifest, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if len(raw) > 4<<20 {
		return nil, ErrLimit
	}
	if !utf8.Valid(raw) {
		return nil, ErrFormat
	}
	d := json.NewDecoder(bytes.NewReader(raw))
	token, err := d.Token()
	if err != nil || token != json.Delim('[') {
		return nil, ErrFormat
	}
	m := &Manifest{tables: make(map[string]TableIdentity)}
	modern, legacy := make(map[uint32]bool), make(map[uint32]bool)
	for d.More() {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		if len(m.tables) >= 16384 {
			return nil, ErrLimit
		}
		token, err = d.Token()
		if err != nil || token != json.Delim('{') {
			return nil, ErrFormat
		}
		seen := make(map[string]bool)
		var entry TableIdentity
		for d.More() {
			token, err = d.Token()
			name, ok := token.(string)
			if err != nil || !ok || seen[name] {
				return nil, ErrFormat
			}
			seen[name] = true
			var value json.RawMessage
			if err := d.Decode(&value); err != nil || bytes.Equal(value, []byte("null")) {
				return nil, ErrFormat
			}
			switch name {
			case "tableName":
				if json.Unmarshal(value, &entry.Name) != nil || !tableNamePattern.MatchString(entry.Name) {
					return nil, ErrFormat
				}
			case "tableHash":
				var hash string
				if json.Unmarshal(value, &hash) != nil || len(hash) != 8 {
					return nil, ErrFormat
				}
				n, err := strconv.ParseUint(hash, 16, 32)
				if err != nil {
					return nil, ErrFormat
				}
				entry.Hash = uint32(n)
			case "db2FileDataID", "dbcFileDataID":
				var id uint32
				if json.Unmarshal(value, &id) != nil || id == 0 {
					return nil, ErrFormat
				}
				if name == "db2FileDataID" {
					entry.DB2FileDataID = id
				} else {
					entry.DBCFileDataID = id
				}
			default:
				return nil, ErrFormat
			}
		}
		token, err = d.Token()
		if err != nil || token != json.Delim('}') || !seen["tableName"] || !seen["tableHash"] {
			return nil, ErrFormat
		}
		key := strings.ToLower(entry.Name)
		if _, exists := m.tables[key]; exists {
			return nil, ErrFormat
		}
		if entry.DB2FileDataID != 0 {
			if modern[entry.DB2FileDataID] {
				return nil, ErrFormat
			}
			modern[entry.DB2FileDataID] = true
		}
		if entry.DBCFileDataID != 0 {
			if legacy[entry.DBCFileDataID] {
				return nil, ErrFormat
			}
			legacy[entry.DBCFileDataID] = true
		}
		m.tables[key] = entry
	}
	token, err = d.Token()
	if err != nil || token != json.Delim(']') || len(m.tables) == 0 {
		return nil, ErrFormat
	}
	if _, err := d.Token(); err != io.EOF {
		return nil, ErrFormat
	}
	sum := sha256.Sum256(raw)
	m.digest = hex.EncodeToString(sum[:])
	return m, nil
}

func (m *Manifest) SHA256() string { return m.digest }

// SelectHash is for formats such as XFTH that carry no file ID. A colliding
// name hash cannot authenticate a table even when a caller supplies its name.
func (m *Manifest) SelectHash(ctx context.Context, hash uint32) (TableIdentity, error) {
	var selected TableIdentity
	found := false
	for _, entry := range m.tables {
		if err := ctx.Err(); err != nil {
			return TableIdentity{}, err
		}
		if entry.Hash != hash {
			continue
		}
		if found {
			return TableIdentity{}, ErrAmbiguous
		}
		selected, found = entry, true
	}
	if !found {
		return TableIdentity{}, ErrMissing
	}
	return selected, nil
}

// Lookup selects the canonical table name and file ID without claiming that a
// subsequently loaded WDC file matches. Resolve must still verify its hash.
func (m *Manifest) Lookup(ctx context.Context, name string) (TableIdentity, error) {
	if err := ctx.Err(); err != nil {
		return TableIdentity{}, err
	}
	if !tableNamePattern.MatchString(name) {
		return TableIdentity{}, ErrFormat
	}
	entry, exists := m.tables[strings.ToLower(name)]
	if !exists {
		return TableIdentity{}, ErrMissing
	}
	return entry, nil
}

// Resolve verifies all three independent identifiers before selecting a DBD.
// It never substitutes a legacy DBC ID, nor resolves by a colliding hash alone.
func (m *Manifest) Resolve(ctx context.Context, name string, fileID, tableHash uint32) (TableIdentity, error) {
	if err := ctx.Err(); err != nil {
		return TableIdentity{}, err
	}
	if !tableNamePattern.MatchString(name) || fileID == 0 {
		return TableIdentity{}, ErrFormat
	}
	entry, err := m.Lookup(ctx, name)
	if err != nil {
		return TableIdentity{}, err
	}
	if entry.DB2FileDataID != fileID || entry.Hash != tableHash {
		return TableIdentity{}, ErrFormat
	}
	return entry, nil
}
