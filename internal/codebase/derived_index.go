package codebase

import (
	"bufio"
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
)

const derivedSchema = "lycheedev.source-offsets.v1"
const maxDerivedBytes = 128 << 20
const maxDerivedRecords = 1000000

// Metadata is bounded and contains no full facts or document bodies. Offsets
// preserve original record order; each selected record is checked again before
// decoding, even if another process rewrites the already-open file.
type recordOffset struct {
	Offset     int64  `json:"o"`
	Length     int    `json:"l"`
	Hash       string `json:"h"`
	Kind       string `json:"k"`
	Path       string `json:"p"`
	SymbolKind string `json:"s,omitempty"`
	Name       string `json:"n,omitempty"`
	Target     string `json:"t,omitempty"`
	ID         string `json:"i,omitempty"`
	Category   string `json:"c,omitempty"`
	Signature  string `json:"g,omitempty"`
	Line       int    `json:"r,omitempty"`
}
type derivedIndex struct {
	Schema      string         `json:"schema"`
	RecordsHash string         `json:"recordsHash"`
	Entries     []recordOffset `json:"entries"`
}

func offsetRecord(r sourceRecord, raw []byte, offset int64) recordOffset {
	h := sha256.Sum256(raw)
	e := recordOffset{Offset: offset, Length: len(raw), Hash: hex.EncodeToString(h[:]), Kind: r.Kind, Path: r.Path}
	if r.Symbol != nil {
		e.SymbolKind, e.Name, e.Target, e.ID = r.Symbol.Kind, r.Symbol.Name, r.Symbol.Target, r.Symbol.ID
		e.Category, e.Signature = r.Symbol.Category, r.Symbol.Signature
		e.Line = r.Symbol.Line
	}
	// Parser strings may be substrings of a much larger input buffer.
	e.Path, e.Name, e.Target, e.Signature = strings.Clone(e.Path), strings.Clone(e.Name), strings.Clone(e.Target), strings.Clone(e.Signature)
	return e
}

func (c *snapshotCache) loadDerived(ctx context.Context) error {
	if c.manifest.DerivedHash == "" {
		if c.manifest.DerivedSchema != "" || c.manifest.DerivedBytes != 0 {
			return fmt.Errorf("%w: codebase.index_manifest_invalid", ErrSourceIntegrity)
		}
		if c.manifest.DerivedState == "budget" {
			return nil
		}
		if c.manifest.DerivedState != "" {
			return fmt.Errorf("%w: codebase.index_manifest_invalid", ErrSourceIntegrity)
		}
		if c.derivedRebuilt {
			return nil
		}
		return c.rebuildOffsets(ctx)
	}
	if c.manifest.DerivedState != "" {
		return fmt.Errorf("%w: codebase.index_manifest_invalid", ErrSourceIntegrity)
	}
	if len(c.manifest.DerivedHash) != 64 || c.manifest.DerivedSchema != derivedSchema || c.manifest.DerivedBytes < 1 || c.manifest.DerivedBytes > maxDerivedBytes {
		return fmt.Errorf("%w: codebase.index_manifest_invalid", ErrSourceIntegrity)
	}
	f, err := os.Open(filepath.Join(c.dir, "offsets.json"))
	if errors.Is(err, os.ErrNotExist) {
		return c.rebuildOffsets(ctx)
	}
	if err != nil {
		return err
	}
	defer f.Close()
	raw, err := io.ReadAll(io.LimitReader(&contextReader{ctx: ctx, reader: f}, c.manifest.DerivedBytes+1))
	if err != nil {
		return err
	}
	h := sha256.Sum256(raw)
	if int64(len(raw)) != c.manifest.DerivedBytes || hex.EncodeToString(h[:]) != c.manifest.DerivedHash {
		return fmt.Errorf("%w: codebase.index_content_mismatch", ErrSourceIntegrity)
	}
	d, err := decodeDerived(ctx, raw)
	if err != nil {
		return fmt.Errorf("%w: codebase.index_record_invalid: %v", ErrSourceIntegrity, err)
	}
	if d.Schema != derivedSchema || d.RecordsHash != c.manifest.RecordsHash || len(d.Entries) > maxDerivedRecords {
		return fmt.Errorf("%w: codebase.index_identity_mismatch", ErrSourceIntegrity)
	}
	var end int64
	for _, e := range d.Entries {
		if err := ctx.Err(); err != nil {
			return err
		}
		if e.Offset != end || e.Length < 1 || e.Length >= 1<<20 || len(e.Hash) != 64 {
			return fmt.Errorf("%w: codebase.index_record_invalid", ErrSourceIntegrity)
		}
		end += int64(e.Length) + 1
	}
	if end != c.manifest.RecordBytes {
		return fmt.Errorf("%w: codebase.index_content_mismatch", ErrSourceIntegrity)
	}
	c.offsets = d.Entries
	return nil
}

// Decode the bounded entries stream rather than allocating an attacker-sized
// slice before checking the count. Unknown and duplicate fields fail closed.
func decodeDerived(ctx context.Context, raw []byte) (derivedIndex, error) {
	var d derivedIndex
	decoder := json.NewDecoder(bytes.NewReader(raw))
	token, err := decoder.Token()
	if err != nil || token != json.Delim('{') {
		return d, errors.New("invalid offsets object")
	}
	seen := map[string]bool{}
	for decoder.More() {
		if err := ctx.Err(); err != nil {
			return d, err
		}
		token, err := decoder.Token()
		if err != nil {
			return d, err
		}
		key, ok := token.(string)
		if !ok || seen[key] {
			return d, errors.New("invalid offsets field")
		}
		seen[key] = true
		switch key {
		case "schema":
			err = decoder.Decode(&d.Schema)
		case "recordsHash":
			err = decoder.Decode(&d.RecordsHash)
		case "entries":
			token, err = decoder.Token()
			if err != nil || token != json.Delim('[') {
				return d, errors.New("invalid offsets entries")
			}
			for decoder.More() {
				if err := ctx.Err(); err != nil {
					return d, err
				}
				if len(d.Entries) >= maxDerivedRecords {
					return d, errors.New("offsets entry budget")
				}
				var e recordOffset
				if err = decoder.Decode(&e); err != nil {
					return d, err
				}
				d.Entries = append(d.Entries, e)
			}
			_, err = decoder.Token()
		default:
			return d, errors.New("unknown offsets field")
		}
		if err != nil {
			return d, err
		}
	}
	if _, err = decoder.Token(); err != nil {
		return d, err
	}
	if _, err = decoder.Token(); err != io.EOF {
		return d, errors.New("trailing offsets data")
	}
	return d, nil
}

// Missing sidecars and pre-offset manifests rebuild only from the fixed,
// verified handle. No disk publication or retired database fallback is used.
func (c *snapshotCache) rebuildOffsets(ctx context.Context) error {
	entries := []recordOffset{}
	var position int64
	bytes := 0
	overflow := false
	err := c.scanLegacyRaw(ctx, func(r sourceRecord, raw []byte) error {
		if !overflow {
			e := offsetRecord(r, raw, position)
			encoded, err := json.Marshal(e)
			if err != nil {
				return err
			}
			if len(entries) >= maxDerivedRecords || bytes+len(encoded)+1 > maxDerivedBytes-1024 {
				entries = nil
				overflow = true
			} else {
				entries = append(entries, e)
				bytes += len(encoded) + 1
			}
		}
		position += int64(len(raw) + 1)
		return nil
	})
	if err != nil {
		return err
	}
	if position != c.manifest.RecordBytes {
		c.derivedRebuilt = true
		return nil
	} // Legacy final line without newline.
	if !overflow {
		if c.manifest.DerivedHash != "" {
			raw, err := json.Marshal(derivedIndex{Schema: derivedSchema, RecordsHash: c.manifest.RecordsHash, Entries: entries})
			if err != nil {
				return err
			}
			h := sha256.Sum256(raw)
			if int64(len(raw)) != c.manifest.DerivedBytes || hex.EncodeToString(h[:]) != c.manifest.DerivedHash {
				return fmt.Errorf("%w: codebase.index_content_mismatch", ErrSourceIntegrity)
			}
		}
		c.offsets = entries
	}
	c.derivedRebuilt = true
	return nil
}

type contextReader struct {
	ctx    context.Context
	reader io.Reader
}

func (r *contextReader) Read(p []byte) (int, error) {
	if err := r.ctx.Err(); err != nil {
		return 0, err
	}
	return r.reader.Read(p)
}

func (c *snapshotCache) scanSelected(ctx context.Context, selectRecord func(recordOffset) bool, visit func(sourceRecord) error) error {
	if c.offsets == nil {
		return c.scanLegacy(ctx, visit)
	}
	for _, e := range c.offsets {
		c.work.MetadataVisits++
		if err := ctx.Err(); err != nil {
			return err
		}
		if !selectRecord(e) {
			continue
		}
		raw := make([]byte, e.Length+1)
		c.work.RecordReadBytes += int64(len(raw))
		if _, err := c.records.ReadAt(raw, e.Offset); err != nil {
			return err
		}
		if raw[len(raw)-1] != '\n' {
			return fmt.Errorf("%w: codebase.index_content_mismatch", ErrSourceIntegrity)
		}
		raw = raw[:e.Length]
		h := sha256.Sum256(raw)
		if hex.EncodeToString(h[:]) != e.Hash {
			return fmt.Errorf("%w: codebase.index_content_mismatch", ErrSourceIntegrity)
		}
		var r sourceRecord
		c.work.DecodedRecords++
		if err := json.Unmarshal(raw, &r); err != nil {
			return fmt.Errorf("%w: codebase.index_record_invalid: %v", ErrSourceIntegrity, err)
		}
		if err := visit(r); err != nil {
			return err
		}
	}
	return nil
}

func (c *snapshotCache) scanLegacy(ctx context.Context, visit func(sourceRecord) error) error {
	return c.scanLegacyRaw(ctx, func(r sourceRecord, _ []byte) error { return visit(r) })
}

func (c *snapshotCache) scanLegacyRaw(ctx context.Context, visit func(sourceRecord, []byte) error) error {
	h := sha256.New()
	s := bufio.NewScanner(io.TeeReader(io.NewSectionReader(c.records, 0, c.manifest.RecordBytes), h))
	s.Buffer(make([]byte, 64<<10), 1<<20)
	for s.Scan() {
		if err := ctx.Err(); err != nil {
			return err
		}
		var r sourceRecord
		c.work.DecodedRecords++
		c.work.RecordReadBytes += int64(len(s.Bytes()) + 1)
		if err := json.Unmarshal(s.Bytes(), &r); err != nil {
			return fmt.Errorf("%w: codebase.index_record_invalid: %v", ErrSourceIntegrity, err)
		}
		if err := visit(r, s.Bytes()); err != nil {
			return err
		}
	}
	if err := s.Err(); err != nil {
		return err
	}
	if hex.EncodeToString(h.Sum(nil)) != c.manifest.RecordsHash {
		return fmt.Errorf("%w: codebase.index_content_mismatch", ErrSourceIntegrity)
	}
	return ctx.Err()
}
