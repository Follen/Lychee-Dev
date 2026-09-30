package codebase

import (
	"bufio"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path"
	"path/filepath"
	"sort"
	"strings"
	"sync"
	"unicode/utf8"

	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
)

const indexSchema = "lycheedev.source-map.v2"

type IndexSummary struct {
	Schema           string           `json:"schema"`
	Storage          string           `json:"storage"`
	Repository       string           `json:"repository"`
	Product          string           `json:"product"`
	Commit           string           `json:"commit"`
	Parser           string           `json:"parser"`
	Documents        int              `json:"documents"`
	SkippedDocuments int              `json:"skippedDocuments,omitempty"`
	Declarations     int              `json:"declarations"`
	Relationships    int              `json:"relationships"`
	Assets           int              `json:"assets"`
	Diagnostics      int              `json:"diagnostics"`
	Complete         bool             `json:"complete"`
	DiagnosticSample []FileDiagnostic `json:"diagnosticSample,omitempty"`
}

type SourceCoverage struct {
	AnalyzedDocuments int              `json:"analyzedDocuments"`
	SkippedDocuments  int              `json:"skippedDocuments"`
	Diagnostics       int              `json:"diagnostics"`
	DiagnosticSample  []FileDiagnostic `json:"diagnosticSample,omitempty"`
	Complete          bool             `json:"complete"`
}

func (s IndexSummary) Coverage() SourceCoverage {
	return SourceCoverage{AnalyzedDocuments: s.Documents, SkippedDocuments: s.SkippedDocuments, Diagnostics: s.Diagnostics, DiagnosticSample: s.DiagnosticSample, Complete: s.Complete}
}

type FileDiagnostic struct {
	Path    string `json:"path"`
	Line    int    `json:"line"`
	Message string `json:"message"`
}

type sourceRecord struct {
	Kind       string          `json:"kind"`
	Path       string          `json:"path"`
	Object     string          `json:"object,omitempty"`
	SHA256     string          `json:"sha256,omitempty"`
	Bytes      int64           `json:"bytes,omitempty"`
	Symbol     *SymbolMatch    `json:"symbol,omitempty"`
	Asset      *AssetRow       `json:"asset,omitempty"`
	Diagnostic *FileDiagnostic `json:"diagnostic,omitempty"`
}

type cacheManifest struct {
	Summary       IndexSummary `json:"summary"`
	RecordsHash   string       `json:"recordsHash"`
	RecordBytes   int64        `json:"recordBytes"`
	FixtureRoot   string       `json:"fixtureRoot,omitempty"`
	FixtureDigest string       `json:"fixtureDigest,omitempty"`
	DerivedSchema string       `json:"derivedSchema,omitempty"`
	DerivedHash   string       `json:"derivedHash,omitempty"`
	DerivedBytes  int64        `json:"derivedBytes,omitempty"`
	DerivedState  string       `json:"derivedState,omitempty"`
}

type factEnvelope struct {
	Schema      string        `json:"schema"`
	Parser      string        `json:"parser"`
	Mode        string        `json:"mode"`
	InputSHA256 string        `json:"inputSHA256"`
	FactsSHA256 string        `json:"factsSHA256"`
	Facts       DocumentFacts `json:"facts"`
}

const maxFactCacheBytes = 32 << 20
const maxDocumentMemoFiles = 256

type snapshotCache struct {
	b              *Browser
	pin            selection.SourcePin
	dir            string
	manifest       cacheManifest
	pathsOnce      sync.Once
	documents      map[string]sourceRecord
	assets         map[string]sourceRecord
	pathsErr       error
	records        *os.File
	offsets        []recordOffset
	derivedRebuilt bool
	shared         bool
	memo           map[string][]byte
	memoBytes      int
	batch          *documentBatch
	work           sourceQueryWork
}

type sourceQueryWork struct {
	VerificationPasses int
	VerifiedBytes      int64
	MetadataVisits     int
	DecodedRecords     int
	RecordReadBytes    int64
	DocumentReads      int
	GitBatchStarts     int
}

func (c *snapshotCache) Close() error {
	if c.shared {
		return nil
	}
	return c.closeResources()
}
func (c *snapshotCache) closeResources() error {
	if c.batch != nil {
		c.batch.close()
		c.batch = nil
	}
	if c.records != nil {
		err := c.records.Close()
		c.records = nil
		return err
	}
	return nil
}

func (b *Browser) indexPath(pin selection.SourcePin) string {
	key := sha256.Sum256([]byte(indexSchema + "\x00" + pin.Repository + "\x00" + pin.Product + "\x00" + pin.ExactCommit + "\x00" + pin.ParserRevision))
	return filepath.Join(b.store.Root(), "source", "v1", "facts", "snapshots", hex.EncodeToString(key[:]))
}

func (b *Browser) IndexSource(ctx context.Context, pin selection.SourcePin) (IndexSummary, error) {
	files, err := b.sourceTree(ctx, pin)
	if err != nil {
		return IndexSummary{}, err
	}
	return b.buildIndex(ctx, pin, files, "", "", func(visit func(treeFile, []byte) error) error { return b.visitDocuments(ctx, pin, files, visit) })
}
func (b *Browser) IndexFixture(ctx context.Context, pin selection.SourcePin, root string) (IndexSummary, error) {
	if pin.Repository == "" || pin.Product == "" || !objectID(pin.ExactCommit) || pin.ParserRevision != ParserRevision {
		return IndexSummary{}, errors.New("codebase.invalid_source_pin")
	}
	files, err := directoryTree(root)
	if err != nil {
		return IndexSummary{}, err
	}
	digest, err := fixtureDigest(ctx, root, files)
	if err != nil {
		return IndexSummary{}, err
	}
	return b.buildIndex(ctx, pin, files, root, digest, func(visit func(treeFile, []byte) error) error { return visitDirectory(ctx, root, files, visit) })
}

func fixtureDigest(ctx context.Context, root string, files []treeFile) (string, error) {
	h := sha256.New()
	err := visitDirectory(ctx, root, files, func(file treeFile, data []byte) error {
		fmt.Fprintf(h, "%s\x00%d\x00", file.path, file.size)
		sum := sha256.Sum256(data)
		h.Write(sum[:])
		return nil
	})
	if err != nil {
		return "", err
	}
	for _, file := range files {
		if !treeNeedsRead(file) {
			fmt.Fprintf(h, "%s\x00%d\x00metadata-only\x00", file.path, file.size)
		}
	}
	return hex.EncodeToString(h.Sum(nil)), nil
}

func (b *Browser) buildIndex(ctx context.Context, pin selection.SourcePin, files []treeFile, fixtureRoot, fixtureDigest string, enumerate func(func(treeFile, []byte) error) error) (IndexSummary, error) {
	lease, err := vault.AcquireLease(ctx, filepath.Join(b.store.Root(), "locks"), "source:v1:map:"+pin.Repository+":"+pin.ExactCommit)
	if err != nil {
		return IndexSummary{}, err
	}
	defer lease.Close()
	if existing, summary, err := b.openIndex(ctx, pin); err == nil {
		defer existing.Close()
		if fixtureRoot != "" && existing.manifest.FixtureDigest != fixtureDigest {
			return IndexSummary{}, errors.New("codebase.fixture_changed: frozen source path has different bytes")
		}
		return summary, nil
	} else if !errors.Is(err, errIndexNotReady) {
		return IndexSummary{}, err
	}
	destination := b.indexPath(pin)
	capacity, err := b.reserveSourceCapacity(ctx, 8<<20, destination)
	if err != nil {
		return IndexSummary{}, err
	}
	defer capacity.Close()
	var granted, consumed int64 = 8 << 20, 0
	reserve := func(bytes int64) error {
		if bytes < 0 {
			return ErrSourceBudget
		}
		if consumed+bytes > granted {
			more := max(int64(8<<20), consumed+bytes-granted)
			if err := capacity.Grow(ctx, more); err != nil {
				return err
			}
			granted += more
		}
		consumed += bytes
		return nil
	}
	if err := os.MkdirAll(filepath.Dir(destination), 0o755); err != nil {
		return IndexSummary{}, err
	}
	stage, err := os.MkdirTemp(filepath.Dir(destination), ".source-map-")
	if err != nil {
		return IndexSummary{}, err
	}
	defer os.RemoveAll(stage)
	f, err := os.Create(filepath.Join(stage, "records.jsonl"))
	if err != nil {
		return IndexSummary{}, err
	}
	hash := sha256.New()
	w := bufio.NewWriterSize(io.MultiWriter(f, hash), 64<<10)
	offsets := []recordOffset{}
	var recordPosition int64
	var offsetBytes int
	derivedOverflow := false
	write := func(r sourceRecord) error {
		data, err := json.Marshal(r)
		if err != nil {
			return err
		}
		if len(data) > 1<<20 {
			return errors.New("codebase.record_budget")
		}
		if err := reserve(int64(len(data) + 1)); err != nil {
			return err
		}
		if _, err := w.Write(data); err != nil {
			return err
		}
		if !derivedOverflow {
			entry := offsetRecord(r, data, recordPosition)
			encoded, err := json.Marshal(entry)
			if err != nil {
				return err
			}
			if len(offsets) >= maxDerivedRecords || offsetBytes+len(encoded)+1 > maxDerivedBytes-1024 {
				offsets = nil
				derivedOverflow = true
			} else {
				offsets = append(offsets, entry)
				offsetBytes += len(encoded) + 1
			}
		}
		recordPosition += int64(len(data) + 1)
		return w.WriteByte('\n')
	}
	summary := IndexSummary{Schema: indexSchema, Storage: "file-cache", Repository: pin.Repository, Product: pin.Product, Commit: pin.ExactCommit, Parser: pin.ParserRevision, Complete: true}
	visit := func(file treeFile, data []byte) error {
		if err := ctx.Err(); err != nil {
			return err
		}
		if file.asset {
			ext := strings.ToLower(path.Ext(file.path))
			width, height, format := assetImageInfo(data, ext)
			asset := AssetRow{Path: file.path, Bytes: file.size, Extension: ext, MIME: assetMIME(ext), Format: format, Width: width, Height: height, Normalized: normalizeAssetPath(file.path)}
			if data != nil {
				sum := sha256.Sum256(data)
				asset.SHA256Stored = hex.EncodeToString(sum[:])
				asset.ContentHash = asset.SHA256Stored
			}
			if err := write(sourceRecord{Kind: "asset", Path: file.path, Object: file.object, Bytes: file.size, Asset: &asset}); err != nil {
				return err
			}
			summary.Assets++
			return nil
		}
		sum := sha256.Sum256(data)
		digest := hex.EncodeToString(sum[:])
		if err := write(sourceRecord{Kind: "document", Path: file.path, Object: file.object, SHA256: digest, Bytes: file.size}); err != nil {
			return err
		}
		facts, err := b.fileFacts(ctx, file.path, data, digest, reserve)
		if err != nil {
			return err
		}
		for ordinal, d := range facts.Declarations {
			s := SymbolMatch{Kind: "declaration", Name: d.Name, Category: d.Category, Scope: d.Scope, Ordinal: ordinal, Confidence: "exact", Path: file.path, Line: d.Line, EndLine: d.EndLine, Signature: d.Signature}
			s.ID = stableSymbolID(pin, s)
			if err := write(sourceRecord{Kind: "symbol", Path: file.path, Symbol: &s}); err != nil {
				return err
			}
		}
		for _, r := range facts.Relationships {
			s := SymbolMatch{Kind: "relationship", Name: r.From, Target: r.To, Category: r.Category, Confidence: r.Confidence, Path: file.path, Line: r.Line, EndLine: r.Line}
			if err := write(sourceRecord{Kind: "symbol", Path: file.path, Symbol: &s}); err != nil {
				return err
			}
		}
		for _, l := range facts.Loads {
			s := SymbolMatch{Kind: "load", Name: file.path, Target: l.Path, Category: l.Kind, Confidence: "exact", Path: file.path, Line: l.Line, EndLine: l.Line}
			if err := write(sourceRecord{Kind: "symbol", Path: file.path, Symbol: &s}); err != nil {
				return err
			}
		}
		for _, h := range facts.Headers {
			s := SymbolMatch{Kind: "header", Name: h.Key, Target: h.Value, Category: "toc-field", Confidence: "exact", Path: file.path, Line: h.Line, EndLine: h.Line}
			if err := write(sourceRecord{Kind: "symbol", Path: file.path, Symbol: &s}); err != nil {
				return err
			}
		}
		for _, n := range facts.Diagnostics {
			d := FileDiagnostic{Path: file.path, Line: n.Line, Message: n.Message}
			if err := write(sourceRecord{Kind: "diagnostic", Path: file.path, Diagnostic: &d}); err != nil {
				return err
			}
			if len(summary.DiagnosticSample) < 20 {
				summary.DiagnosticSample = append(summary.DiagnosticSample, d)
			}
		}
		summary.Documents++
		summary.Declarations += len(facts.Declarations)
		summary.Relationships += len(facts.Relationships)
		summary.Diagnostics += len(facts.Diagnostics)
		return nil
	}
	if err := enumerate(visit); err != nil {
		f.Close()
		return IndexSummary{}, err
	}
	for _, file := range files {
		if file.oversized {
			d := FileDiagnostic{Path: file.path, Message: fmt.Sprintf("source file has %d bytes, exceeding the %d-byte analysis limit; skipped", file.size, maxSourceBytes)}
			if err := write(sourceRecord{Kind: "diagnostic", Path: file.path, Diagnostic: &d}); err != nil {
				f.Close()
				return IndexSummary{}, err
			}
			summary.SkippedDocuments++
			summary.Diagnostics++
			if len(summary.DiagnosticSample) < 20 {
				summary.DiagnosticSample = append(summary.DiagnosticSample, d)
			}
			continue
		}
		if file.asset && !treeNeedsRead(file) {
			if err := visit(file, nil); err != nil {
				f.Close()
				return IndexSummary{}, err
			}
		}
	}
	if err := w.Flush(); err != nil {
		f.Close()
		return IndexSummary{}, err
	}
	if err := f.Sync(); err != nil {
		f.Close()
		return IndexSummary{}, err
	}
	info, err := f.Stat()
	if err != nil {
		f.Close()
		return IndexSummary{}, err
	}
	if err := f.Close(); err != nil {
		return IndexSummary{}, err
	}
	summary.Complete = summary.Diagnostics == 0
	manifest := cacheManifest{Summary: summary, RecordsHash: hex.EncodeToString(hash.Sum(nil)), RecordBytes: info.Size(), FixtureRoot: fixtureRoot, FixtureDigest: fixtureDigest}
	if !derivedOverflow {
		derivedRaw, err := json.Marshal(derivedIndex{Schema: derivedSchema, RecordsHash: manifest.RecordsHash, Entries: offsets})
		if err != nil {
			return IndexSummary{}, err
		}
		if len(derivedRaw) <= maxDerivedBytes {
			if err := reserve(int64(len(derivedRaw))); err != nil {
				return IndexSummary{}, err
			}
			if err := os.WriteFile(filepath.Join(stage, "offsets.json"), derivedRaw, 0644); err != nil {
				return IndexSummary{}, err
			}
			sum := sha256.Sum256(derivedRaw)
			manifest.DerivedSchema = derivedSchema
			manifest.DerivedHash = hex.EncodeToString(sum[:])
			manifest.DerivedBytes = int64(len(derivedRaw))
		} else {
			manifest.DerivedState = "budget"
		}
	} else {
		manifest.DerivedState = "budget"
	}
	raw, err := json.Marshal(manifest)
	if err != nil {
		return IndexSummary{}, err
	}
	if err := reserve(int64(len(raw))); err != nil {
		return IndexSummary{}, err
	}
	if err := os.WriteFile(filepath.Join(stage, "manifest.json"), raw, 0o644); err != nil {
		return IndexSummary{}, err
	}
	if err := ctx.Err(); err != nil {
		return IndexSummary{}, err
	}
	if err := os.Rename(stage, destination); err != nil {
		return IndexSummary{}, err
	}
	return summary, nil
}

func (b *Browser) fileFacts(ctx context.Context, name string, data []byte, digest string, reserveCallbacks ...func(int64) error) (DocumentFacts, error) {
	mode := strings.ToLower(path.Ext(name))
	if mode == ".lua" && strings.Contains(name, "/Blizzard_APIDocumentationGenerated/") {
		mode += ":generated-api"
	}
	key := sha256.Sum256([]byte(ParserRevision + "\x00" + mode + "\x00" + digest))
	file := filepath.Join(b.store.Root(), "source", "v1", "facts", "files", ParserRevision, hex.EncodeToString(key[:])+".json")
	lease, err := vault.AcquireLease(ctx, filepath.Join(b.store.Root(), "locks"), "source:v1:fact:"+hex.EncodeToString(key[:]))
	if err != nil {
		return DocumentFacts{}, err
	}
	defer lease.Close()
	if f, err := os.Open(file); err == nil {
		raw, readErr := io.ReadAll(io.LimitReader(f, maxFactCacheBytes+1))
		closeErr := f.Close()
		if readErr == nil && closeErr == nil && len(raw) <= maxFactCacheBytes {
			var envelope factEnvelope
			if json.Unmarshal(raw, &envelope) == nil && envelope.Schema == "lycheedev.source-file-facts.v2" && envelope.Parser == ParserRevision && envelope.Mode == mode && envelope.InputSHA256 == digest {
				payload, _ := json.Marshal(envelope.Facts)
				sum := sha256.Sum256(payload)
				if envelope.FactsSHA256 == hex.EncodeToString(sum[:]) {
					return envelope.Facts, nil
				}
			}
		}
	}
	facts, err := AnalyzeDocument(ctx, name, data)
	if err != nil {
		return facts, err
	}
	payload, err := json.Marshal(facts)
	if err != nil {
		return facts, err
	}
	sum := sha256.Sum256(payload)
	raw, err := json.Marshal(factEnvelope{Schema: "lycheedev.source-file-facts.v2", Parser: ParserRevision, Mode: mode, InputSHA256: digest, FactsSHA256: hex.EncodeToString(sum[:]), Facts: facts})
	if err != nil {
		return facts, err
	}
	if len(raw) > maxFactCacheBytes {
		return facts, errors.New("codebase.fact_budget")
	}
	// The index writer owns the source-capacity lease. Read-only standalone
	// analysis may reuse an existing fact cache but must not publish new bytes
	// while holding the fact-key lease without that shared reservation.
	if len(reserveCallbacks) == 0 {
		return facts, nil
	}
	if err := reserveCallbacks[0](int64(len(raw))); err != nil {
		return facts, err
	}
	if err := os.MkdirAll(filepath.Dir(file), 0o755); err != nil {
		return facts, err
	}
	tmp, err := os.CreateTemp(filepath.Dir(file), ".fact-")
	if err != nil {
		return facts, err
	}
	defer os.Remove(tmp.Name())
	if _, err := tmp.Write(raw); err != nil {
		tmp.Close()
		return facts, err
	}
	if err := tmp.Close(); err != nil {
		return facts, err
	}
	if _, err := os.Stat(file); err == nil {
		if err := os.Remove(file); err != nil {
			return facts, err
		}
	}
	if err := os.Rename(tmp.Name(), file); err != nil {
		return facts, err
	}
	return facts, nil
}

var errIndexNotReady = errors.New("codebase.index_not_ready: run source index with the same snapshot")

func (b *Browser) openIndex(ctx context.Context, pin selection.SourcePin) (*snapshotCache, IndexSummary, error) {
	dir := b.indexPath(pin)
	session, _ := ctx.Value(sourceQueryKey{}).(*sourceQuerySession)
	key := b.store.Root() + "\x00" + dir
	if session != nil {
		session.mu.Lock()
		defer session.mu.Unlock()
		if c := session.caches[key]; c != nil {
			if err := ctx.Err(); err != nil {
				return nil, IndexSummary{}, err
			}
			return c, c.manifest.Summary, nil
		}
	}
	f, err := os.Open(filepath.Join(dir, "manifest.json"))
	if errors.Is(err, os.ErrNotExist) {
		return nil, IndexSummary{}, errIndexNotReady
	}
	if err != nil {
		return nil, IndexSummary{}, err
	}
	raw, err := io.ReadAll(io.LimitReader(f, 64<<10))
	closeErr := f.Close()
	if err != nil || closeErr != nil {
		return nil, IndexSummary{}, errors.Join(err, closeErr)
	}
	var m cacheManifest
	if err := json.Unmarshal(raw, &m); err != nil {
		return nil, IndexSummary{}, err
	}
	if m.Summary.Schema != indexSchema || m.Summary.Storage != "file-cache" || m.Summary.Repository != pin.Repository || m.Summary.Product != pin.Product || m.Summary.Commit != pin.ExactCommit || m.Summary.Parser != pin.ParserRevision {
		return nil, IndexSummary{}, errors.New("codebase.index_identity_mismatch")
	}
	if len(m.RecordsHash) != 64 || m.RecordBytes < 0 || m.RecordBytes > 1<<30 {
		return nil, IndexSummary{}, errors.New("codebase.index_manifest_invalid")
	}
	cache := &snapshotCache{b: b, pin: pin, dir: dir, manifest: m}
	if err := cache.verify(ctx); err != nil {
		return nil, IndexSummary{}, err
	}
	if err := cache.loadDerived(ctx); err != nil {
		cache.closeResources()
		return nil, IndexSummary{}, err
	}
	if session != nil && len(session.caches) < 4 {
		cache.shared = true
		session.caches[key] = cache
	}
	return cache, m.Summary, nil
}
func (c *snapshotCache) verify(ctx context.Context) error {
	c.work.VerificationPasses++
	f, err := os.Open(filepath.Join(c.dir, "records.jsonl"))
	if err != nil {
		return err
	}
	success := false
	defer func() {
		if !success {
			f.Close()
		}
	}()
	info, err := f.Stat()
	if err != nil {
		return err
	}
	if info.Size() != c.manifest.RecordBytes {
		return fmt.Errorf("%w: codebase.index_content_mismatch", ErrSourceIntegrity)
	}
	if c.manifest.DerivedHash == "" && c.manifest.DerivedState == "" {
		c.records = f
		if err := c.rebuildOffsets(ctx); err != nil {
			c.records = nil
			return err
		}
		c.work.VerifiedBytes += info.Size()
		success = true
		return nil
	}
	h := sha256.New()
	if _, err := io.CopyBuffer(h, &contextReader{ctx: ctx, reader: f}, make([]byte, 64<<10)); err != nil {
		return err
	}
	if err := ctx.Err(); err != nil {
		return err
	}
	if hex.EncodeToString(h.Sum(nil)) != c.manifest.RecordsHash {
		return fmt.Errorf("%w: codebase.index_content_mismatch", ErrSourceIntegrity)
	}
	c.work.VerifiedBytes += info.Size()
	c.records = f
	success = true
	return nil
}
func (c *snapshotCache) scan(ctx context.Context, visit func(sourceRecord) error) error {
	return c.scanSelected(ctx, func(recordOffset) bool { return true }, visit)
}

func (c *snapshotCache) loadPaths(ctx context.Context) error {
	c.pathsOnce.Do(func() {
		c.documents = map[string]sourceRecord{}
		c.assets = map[string]sourceRecord{}
		c.pathsErr = c.scanSelected(ctx, func(e recordOffset) bool { return e.Kind == "document" || e.Kind == "asset" }, func(r sourceRecord) error {
			if (r.Kind == "document" || r.Kind == "asset") && len(c.documents)+len(c.assets) >= maxSourceNodes {
				return ErrSourceBudget
			}
			switch r.Kind {
			case "document":
				c.documents[r.Path] = r
			case "asset":
				c.assets[r.Path] = r
			}
			return nil
		})
	})
	return c.pathsErr
}

func (c *snapshotCache) document(ctx context.Context, name string) ([]byte, string, error) {
	if !sourcePath(name) {
		return nil, "", errors.New("codebase.invalid_span")
	}
	if c.offsets != nil {
		if c.documents == nil {
			c.documents = map[string]sourceRecord{}
		}
		if _, ok := c.documents[name]; !ok {
			if err := c.scanSelected(ctx, func(e recordOffset) bool { return e.Kind == "document" && e.Path == name }, func(r sourceRecord) error {
				if r.Kind == "document" && r.Path == name {
					if len(c.documents) >= maxSourceNodes {
						return ErrSourceBudget
					}
					c.documents[name] = r
				}
				return nil
			}); err != nil {
				return nil, "", err
			}
		}
	} else if err := c.loadPaths(ctx); err != nil {
		return nil, "", err
	}
	found, ok := c.documents[name]
	if !ok {
		return nil, "", os.ErrNotExist
	}
	if found.Bytes < 0 || found.Bytes > maxSourceBytes {
		return nil, "", errors.New("codebase.source_byte_limit")
	}
	memoKey := found.Object + "\x00" + found.SHA256
	if data, ok := c.memo[memoKey]; ok {
		if err := ctx.Err(); err != nil {
			return nil, "", err
		}
		return data, found.SHA256, nil
	}
	var data []byte
	c.work.DocumentReads++
	var err error
	if c.manifest.FixtureRoot != "" {
		full := filepath.Join(c.manifest.FixtureRoot, filepath.FromSlash(name))
		info, err := os.Lstat(full)
		if err != nil {
			return nil, "", err
		}
		if !info.Mode().IsRegular() {
			return nil, "", errors.New("codebase.not_regular_source_file")
		}
		f, err := os.Open(full)
		if err != nil {
			return nil, "", err
		}
		data, err = io.ReadAll(io.LimitReader(f, found.Bytes+1))
		closeErr := f.Close()
		if err != nil || closeErr != nil {
			return nil, "", errors.Join(err, closeErr)
		}
	} else {
		if !objectID(found.Object) {
			return nil, "", errors.New("codebase.index_record_invalid")
		}
		if c.batch == nil {
			c.work.GitBatchStarts++
			c.batch, err = newDocumentBatch(ctx, c.b, c.pin)
		}
		if err == nil {
			data, err = c.batch.read(found.Object, found.Bytes)
		}
		if err != nil {
			return nil, "", err
		}
	}
	sum := sha256.Sum256(data)
	digest := hex.EncodeToString(sum[:])
	if int64(len(data)) != found.Bytes || digest != found.SHA256 || !utf8.Valid(data) {
		return nil, "", errors.New("codebase.source_content_mismatch")
	}
	if len(data) <= 64<<20 {
		if c.memo == nil {
			c.memo = map[string][]byte{}
		}
		if c.memoBytes+len(data) > 64<<20 || len(c.memo) >= maxDocumentMemoFiles {
			clear(c.memo)
			c.memoBytes = 0
		}
		c.memo[memoKey] = data
		c.memoBytes += len(data)
	}
	return data, digest, nil
}

type SymbolMatch struct {
	ID         string `json:"id,omitempty"`
	Kind       string `json:"kind"`
	Name       string `json:"name"`
	Scope      string `json:"scope,omitempty"`
	Ordinal    int    `json:"ordinal,omitempty"`
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

func stableSymbolID(pin selection.SourcePin, s SymbolMatch) string {
	h := sha256.Sum256([]byte(pin.Repository + "\x00" + pin.ExactCommit + "\x00" + s.Path + "\x00" + fmt.Sprint(s.Line) + "\x00" + fmt.Sprint(s.EndLine) + "\x00" + s.Name + "\x00" + s.Category + "\x00" + s.Scope + "\x00" + fmt.Sprint(s.Ordinal)))
	return "SYM-" + hex.EncodeToString(h[:16])
}
func (b *Browser) FindSymbols(ctx context.Context, pin selection.SourcePin, term string, limit int) (SymbolMatches, error) {
	ctx, closeQuery := sourceQueryContext(ctx)
	defer closeQuery()

	result := SymbolMatches{Matches: []SymbolMatch{}}
	if term == "" || len(term) > 512 || limit < 1 || limit > 200 {
		return result, errors.New("codebase.invalid_symbol_query")
	}
	cache, summary, err := b.openIndex(ctx, pin)
	if err != nil {
		return result, err
	}
	defer cache.Close()
	result.Index = summary
	var matches []SymbolMatch
	err = cache.scanSelected(ctx, func(e recordOffset) bool { return e.Kind == "symbol" && (e.Name == term || e.Target == term) }, func(r sourceRecord) error {
		if r.Kind == "symbol" && r.Symbol != nil && (r.Symbol.Name == term || r.Symbol.Target == term) {
			matches = append(matches, *r.Symbol)
		}
		return nil
	})
	if err != nil {
		return result, err
	}
	sort.Slice(matches, func(i, j int) bool {
		a, z := matches[i], matches[j]
		if a.Kind != z.Kind {
			return a.Kind < z.Kind
		}
		if a.Path != z.Path {
			return a.Path < z.Path
		}
		if a.Line != z.Line {
			return a.Line < z.Line
		}
		return a.Name < z.Name
	})
	result.Truncated = len(matches) > limit
	result.Matches = matches[:min(limit, len(matches))]
	return result, nil
}
