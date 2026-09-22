// SPDX-License-Identifier: AGPL-3.0-or-later
// Listfile formats adapted from wowdata; see THIRD_PARTY_NOTICES.md.
package records

import (
	"bytes"
	"encoding/binary"
	"errors"
	"fmt"
	"sort"
	"strconv"
	"strings"
	"time"

	"github.com/follenfang/lycheedev/internal/vault"
)

// ListfileKind names one explicit community-maintained filename source and its
// exact serialization. Sources are never silently switched or merged: a caller
// that wants another format asks for it and receives its own provenance.
type ListfileKind string

const (
	ListfileCommunityCSV    ListfileKind = "community-csv"
	ListfileWowExportText   ListfileKind = "wowexport-text"
	ListfileWowExportBinary ListfileKind = "wowexport-binary"
)

var (
	// ErrListfileUnavailable means the requested listfile source could not be
	// prepared at all. It is distinct from "no entry for this query".
	ErrListfileUnavailable = errors.New("records.listfile_unavailable")
	ErrListfileFormat      = errors.New("records.listfile_format")
	ErrListfileLimit       = errors.New("records.listfile_limit")
	ErrListfileQuery       = errors.New("records.listfile_query")
)

// ListfileLimits bounds raw payload bytes, parsed entries and one physical
// line/record. Zero values select bounded defaults; negative or oversized
// values are rejected rather than silently raised.
type ListfileLimits struct {
	Bytes     int64
	Entries   int
	LineBytes int64
}

const (
	listfileDefaultBytes     = 64 << 20
	listfileDefaultEntries   = 4 << 20
	listfileDefaultLineBytes = 1 << 20
	listfileMaxBytes         = 1 << 30
	listfileMaxEntries       = 16 << 20
	listfileMaxLineBytes     = 16 << 20
)

func normalizeListfileLimits(limits ListfileLimits) (ListfileLimits, error) {
	if limits.Bytes == 0 {
		limits.Bytes = listfileDefaultBytes
	}
	if limits.Entries == 0 {
		limits.Entries = listfileDefaultEntries
	}
	if limits.LineBytes == 0 {
		limits.LineBytes = listfileDefaultLineBytes
	}
	if limits.Bytes < 1024 || limits.Bytes > listfileMaxBytes ||
		limits.Entries < 1 || limits.Entries > listfileMaxEntries ||
		limits.LineBytes < 128 || limits.LineBytes > listfileMaxLineBytes {
		return ListfileLimits{}, ErrListfileLimit
	}
	return limits, nil
}

// ListfileSourceFile records one downloaded payload with its exact locator and
// digest, so a cached listfile can be re-verified and re-attributed later.
type ListfileSourceFile struct {
	Component string        `json:"component"`
	URL       string        `json:"url"`
	Bytes     int64         `json:"bytes"`
	SHA256    string        `json:"sha256"`
	Blob      vault.BlobRef `json:"blob"`
}

// ListfileProvenance is the fixed identity of one prepared listfile: which
// source and files were used, when they were fetched, and what was parsed.
type ListfileProvenance struct {
	Kind       ListfileKind         `json:"kind"`
	Files      []ListfileSourceFile `json:"files"`
	FetchedAt  time.Time            `json:"fetchedAt"`
	EntryCount int                  `json:"entryCount"`
	Rejected   int                  `json:"rejected"`
	TotalBytes int64                `json:"totalBytes"`
	Cache      string               `json:"cache"`
}

// ListfileMatch is one filename listing. SourceName preserves the bytes the
// community provided; FileName is the normalized lookup form. Component ties
// the entry back to one file of the source manifest.
type ListfileMatch struct {
	FileDataID uint32 `json:"fileDataID"`
	FileName   string `json:"fileName"`
	SourceName string `json:"sourceName"`
	Component  string `json:"component,omitempty"`
	MatchedVia string `json:"matchedVia,omitempty"`
}

// ListfileIndex is the merged lookup structure of one parsed source: every ID
// keeps every listed name and every name keeps every listed ID. Nothing is
// silently won by ordering.
type ListfileIndex struct {
	provenance ListfileProvenance
	entries    []ListfileMatch
	byID       map[uint32][]int
	byName     map[string][]int
}

func (i *ListfileIndex) Provenance() ListfileProvenance { return i.provenance }
func (i *ListfileIndex) Len() int                       { return len(i.entries) }

// Names returns every listed name for one file ID in listing order.
func (i *ListfileIndex) Names(fileDataID uint32) []ListfileMatch {
	return i.matches(i.byID[fileDataID])
}

// IDs returns every listed file ID for one exact normalized name.
func (i *ListfileIndex) IDs(name string) []ListfileMatch {
	return i.matches(i.byName[normalizeListfileName(name)])
}

// ResolveName reports every ID listed for the name. When no exact listing
// exists and the name is a legacy model alias (.mdl/.mdx), the .m2 listing is
// reported with MatchedVia "m2-alias" instead of being returned unmarked.
func (i *ListfileIndex) ResolveName(name string) []ListfileMatch {
	normalized := normalizeListfileName(name)
	if exact := i.matches(i.byName[normalized]); len(exact) > 0 {
		return exact
	}
	if strings.HasSuffix(normalized, ".mdl") || strings.HasSuffix(normalized, ".mdx") {
		alias := i.matches(i.byName[normalized[:len(normalized)-4]+".m2"])
		for n := range alias {
			alias[n].MatchedVia = "m2-alias"
		}
		return alias
	}
	return nil
}

// Search returns the total match count and up to limit matches in listing
// order. The query is a case-insensitive substring of the normalized name.
func (i *ListfileIndex) Search(query string, limit int) (int, []ListfileMatch) {
	needle := strings.ToLower(query)
	total := 0
	var matches []ListfileMatch
	for _, entry := range i.entries {
		if !strings.Contains(entry.FileName, needle) {
			continue
		}
		total++
		if len(matches) < limit {
			matches = append(matches, entry)
		}
	}
	return total, matches
}

// ByExtension returns the total count and up to limit entries whose normalized
// name ends in the extension, ordered by name then file ID.
func (i *ListfileIndex) ByExtension(extension string, limit int) (int, []ListfileMatch) {
	suffix := "." + strings.ToLower(strings.TrimPrefix(strings.TrimSpace(extension), "."))
	total := 0
	var sorted []ListfileMatch
	for _, entry := range i.entries {
		if !strings.HasSuffix(entry.FileName, suffix) {
			continue
		}
		total++
		sorted = append(sorted, entry)
	}
	sort.Slice(sorted, func(a, b int) bool {
		if sorted[a].FileName != sorted[b].FileName {
			return sorted[a].FileName < sorted[b].FileName
		}
		return sorted[a].FileDataID < sorted[b].FileDataID
	})
	if len(sorted) > limit {
		sorted = sorted[:limit]
	}
	return total, sorted
}

func (i *ListfileIndex) matches(indexes []int) []ListfileMatch {
	if len(indexes) == 0 {
		return nil
	}
	result := make([]ListfileMatch, 0, len(indexes))
	for _, n := range indexes {
		result = append(result, i.entries[n])
	}
	return result
}

func normalizeListfileName(name string) string {
	return strings.ToLower(strings.ReplaceAll(strings.TrimSpace(name), "\\", "/"))
}

type listfileBuilder struct {
	index    *ListfileIndex
	limits   ListfileLimits
	rejected int
}

func newListfileBuilder(kind ListfileKind, limits ListfileLimits) *listfileBuilder {
	return &listfileBuilder{
		limits: limits,
		index: &ListfileIndex{
			provenance: ListfileProvenance{Kind: kind, Cache: "parsed"},
			byID:       make(map[uint32][]int),
			byName:     make(map[string][]int),
		},
	}
}

// add records one listing. Empty or control-laden names are counted as
// rejected instead of being guessed at.
func (b *listfileBuilder) add(fileDataID uint32, sourceName, component string) error {
	if fileDataID == 0 || sourceName == "" || strings.IndexByte(sourceName, 0) >= 0 {
		b.rejected++
		return nil
	}
	if len(b.index.entries) >= b.limits.Entries {
		return ErrListfileLimit
	}
	normalized := normalizeListfileName(sourceName)
	if normalized == "" {
		b.rejected++
		return nil
	}
	position := len(b.index.entries)
	b.index.entries = append(b.index.entries, ListfileMatch{
		FileDataID: fileDataID,
		FileName:   normalized,
		SourceName: sourceName,
		Component:  component,
	})
	b.index.byID[fileDataID] = append(b.index.byID[fileDataID], position)
	b.index.byName[normalized] = append(b.index.byName[normalized], position)
	return nil
}

func (b *listfileBuilder) done() (*ListfileIndex, error) {
	b.index.provenance.EntryCount = len(b.index.entries)
	b.index.provenance.Rejected = b.rejected
	return b.index, nil
}

// ParseTextListfile parses the community CSV and wow.export text listfiles:
// one "fileDataID;filename" listing per line. Blank lines, "#" comments, a
// UTF-8 BOM, CRLF ends and malformed lines are tolerated and counted, matching
// the tolerant legacy reader, but malformed input never becomes a name guess.
func ParseTextListfile(kind ListfileKind, data []byte, limits ListfileLimits) (*ListfileIndex, error) {
	if kind != ListfileCommunityCSV && kind != ListfileWowExportText {
		return nil, ErrListfileQuery
	}
	limits, err := normalizeListfileLimits(limits)
	if err != nil {
		return nil, err
	}
	if int64(len(data)) > limits.Bytes {
		return nil, ErrListfileLimit
	}
	builder := newListfileBuilder(kind, limits)
	builder.index.provenance.TotalBytes = int64(len(data))
	body := bytes.TrimPrefix(data, []byte{0xEF, 0xBB, 0xBF})
	for offset := 0; offset < len(body); {
		end := len(body)
		if n := bytes.IndexByte(body[offset:], '\n'); n >= 0 {
			end = offset + n
		}
		line := strings.TrimSuffix(string(body[offset:end]), "\r")
		offset = min(end+1, len(body))
		if len(line) > int(limits.LineBytes) {
			builder.rejected++
			continue
		}
		text := strings.TrimSpace(line)
		if text == "" || strings.HasPrefix(text, "#") {
			continue
		}
		id, name, ok := splitListfileLine(text)
		if !ok {
			builder.rejected++
			continue
		}
		if err := builder.add(id, name, ""); err != nil {
			return nil, err
		}
	}
	return builder.done()
}

// splitListfileLine separates one "id;name" (or "id,name") listing. The
// separator is the first ";" when present, else the first ",".
func splitListfileLine(text string) (uint32, string, bool) {
	separator := strings.IndexByte(text, ';')
	if separator < 0 {
		separator = strings.IndexByte(text, ',')
	}
	if separator < 0 {
		return 0, "", false
	}
	rawID := strings.TrimSpace(text[:separator])
	name := strings.TrimSpace(text[separator+1:])
	if rawID == "" || name == "" {
		return 0, "", false
	}
	for _, c := range rawID {
		if c < '0' || c > '9' {
			return 0, "", false
		}
	}
	id, err := strconv.ParseUint(rawID, 10, 32)
	if err != nil || id == 0 {
		return 0, "", false
	}
	name = unquoteListfileName(name)
	if name == "" {
		return 0, "", false
	}
	return uint32(id), name, true
}

// unquoteListfileName removes one optional CSV quoting layer without applying
// escape semantics the community format does not use.
func unquoteListfileName(name string) string {
	if len(name) < 2 || name[0] != '"' || name[len(name)-1] != '"' {
		return name
	}
	return strings.ReplaceAll(name[1:len(name)-1], `""`, `"`)
}

// Binary listfile components in wow.export's pf-index order. The componentized
// binary listfile stores a 9-byte ID index (big-endian fileDataID, big-endian
// string offset, one string-file index) plus NUL-terminated name pools. The
// optional tree-node acceleration file is an internal search structure and is
// not needed once entries are merged into ListfileIndex.
var binaryListfileStringComponents = []string{
	"listfile-strings.dat",
	"listfile-pf-models.dat",
	"listfile-pf-textures.dat",
	"listfile-pf-sounds.dat",
	"listfile-pf-videos.dat",
	"listfile-pf-text.dat",
	"listfile-pf-fonts.dat",
}

const binaryListfileIndexComponent = "listfile-id-index.dat"

// BinaryListfileComponents lists every payload the binary source provides.
func BinaryListfileComponents() []string {
	return append([]string{binaryListfileIndexComponent}, binaryListfileStringComponents...)
}

// ParseBinaryListfile parses wow.export's componentized binary listfile. Every
// referenced name must resolve inside its declared string pool; out-of-range
// offsets, unterminated strings and invalid component indexes are format
// errors, never silent skips.
func ParseBinaryListfile(components map[string][]byte, limits ListfileLimits) (*ListfileIndex, error) {
	limits, err := normalizeListfileLimits(limits)
	if err != nil {
		return nil, err
	}
	var total int64
	for name, data := range components {
		if name == "listfile-tree-nodes.dat" {
			continue
		}
		total += int64(len(data))
	}
	if total > limits.Bytes {
		return nil, ErrListfileLimit
	}
	index, ok := components[binaryListfileIndexComponent]
	if !ok {
		return nil, fmt.Errorf("%w: missing %s", ErrListfileFormat, binaryListfileIndexComponent)
	}
	if len(index) == 0 || len(index)%9 != 0 {
		return nil, fmt.Errorf("%w: id index has invalid length %d", ErrListfileFormat, len(index))
	}
	pools := make([][]byte, len(binaryListfileStringComponents))
	for n, name := range binaryListfileStringComponents {
		data, ok := components[name]
		if !ok {
			return nil, fmt.Errorf("%w: missing %s", ErrListfileFormat, name)
		}
		pools[n] = data
	}
	builder := newListfileBuilder(ListfileWowExportBinary, limits)
	builder.index.provenance.TotalBytes = total
	for offset := 0; offset < len(index); offset += 9 {
		fileDataID := binary.BigEndian.Uint32(index[offset : offset+4])
		stringOffset := binary.BigEndian.Uint32(index[offset+4 : offset+8])
		pool := int(index[offset+8])
		if fileDataID == 0 {
			builder.rejected++
			continue
		}
		if pool >= len(pools) {
			return nil, fmt.Errorf("%w: file %d references invalid component index %d", ErrListfileFormat, fileDataID, pool)
		}
		name, err := readListfileCString(pools[pool], int(stringOffset))
		if err != nil {
			return nil, fmt.Errorf("%w: file %d: %v", ErrListfileFormat, fileDataID, err)
		}
		if err := builder.add(fileDataID, name, binaryListfileStringComponents[pool]); err != nil {
			return nil, err
		}
	}
	return builder.done()
}

func readListfileCString(data []byte, offset int) (string, error) {
	if offset < 0 || offset >= len(data) {
		return "", errors.New("string offset out of range")
	}
	end := offset
	for end < len(data) && data[end] != 0 {
		end++
	}
	if end >= len(data) {
		return "", errors.New("unterminated string")
	}
	return string(data[offset:end]), nil
}
