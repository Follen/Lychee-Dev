package navigatetest

// Synthetic DB2 fixtures. A small builder emits genuine WDC3 bytes (fixed
// rows, inline or external identity, one non-inline relation column, string
// blocks) plus matching DBD text and a WoWDBDefs-style manifest, then stores
// the tables in a fully synthetic authenticated offline CDN root
// (testkit.CachedFiles). Tests therefore run the real FileQuery/ReadFile
// source selection with offline semantics and never touch the network.

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"math"
	"path/filepath"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/records"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/testkit"
	"github.com/follenfang/lycheedev/internal/vault"
)

type fixtureColumn struct {
	name     string
	kind     byte // 'i' int, 'f' float, 's' string/locstring
	bits     int  // 8/16/32/64 for ints
	signed   bool
	elements int // >= 1
	identity bool
	relation bool // non-inline relationship column
	foreign  string
	loc      bool // locstring instead of string
}

type fixtureTable struct {
	name             string
	fileDataID       uint32
	tableHash        uint32
	layoutHash       uint32
	identityExternal bool
	columns          []fixtureColumn
	// rows are aligned with columns; cells are int64, float32, string,
	// []int64 or []float32. Relation cells are int64 or nil (no relationship).
	// The identity cell is the record ID.
	rows [][]any
	// mutate rewrites the WDC bytes before storage (corruption cases).
	mutate func([]byte) []byte
	// raw replaces the built WDC bytes entirely (real binary samples).
	raw []byte
	// object replaces the stored BLTE object (encryption cases).
	object []byte
	// dbdLayout overrides the layout hash the DBD variant binds to.
	dbdLayout uint32
}

type fixture struct {
	t         *testing.T
	workspace string
	pin       selection.PinnedSet
	fileIDs   map[string]uint32
}

func fixtureQuery() records.FileQuery {
	return records.FileQuery{CDN: true, Offline: true, MetadataBytes: 8 << 20, ContentBytes: 32 << 20}
}

func buildTable(t *testing.T, spec fixtureTable) (wdc []byte, dbd string) {
	t.Helper()
	elements := func(col fixtureColumn) int {
		if col.elements > 0 {
			return col.elements
		}
		return 1
	}
	stored := func(col fixtureColumn) bool {
		return !col.relation && !(col.identity && spec.identityExternal)
	}
	stride := 0
	widths := []int{}
	inlineColumns := []int{}
	for index, col := range spec.columns {
		width := 0
		if stored(col) {
			width = 4
			if col.kind == 'i' {
				width = col.bits / 8
			}
			width *= elements(col)
			inlineColumns = append(inlineColumns, index)
		}
		widths = append(widths, width)
		stride += width
	}
	if spec.raw == nil && (stride == 0 || stride > 1<<20) {
		t.Fatalf("bad stride %d", stride)
	}
	var recordsData bytes.Buffer
	var stringBlock bytes.Buffer
	intern := map[string]int{}
	idList := []uint32{}
	type relationEntry struct {
		foreign, ordinal uint32
	}
	relations := []relationEntry{}
	firstID, lastID := uint32(0), uint32(0)
	for rowIndex, row := range spec.rows {
		if len(row) != len(spec.columns) {
			t.Fatalf("row %d has %d cells, want %d", rowIndex, len(row), len(spec.columns))
		}
		id := cellID(t, row[identityIndex(spec)])
		if rowIndex == 0 || id < firstID {
			firstID = id
		}
		if rowIndex == 0 || id > lastID {
			lastID = id
		}
		if spec.identityExternal {
			idList = append(idList, id)
		}
		position := 0
		rowStart := recordsData.Len()
		for index, col := range spec.columns {
			if col.relation {
				if row[index] != nil {
					relations = append(relations, relationEntry{uint32(cellInt(t, row[index])), uint32(rowIndex)})
				}
				continue
			}
			if col.identity && spec.identityExternal {
				continue
			}
			cell := row[index]
			switch col.kind {
			case 's':
				value, ok := cell.(string)
				if !ok {
					t.Fatalf("row %d field %s: string cell required", rowIndex, col.name)
				}
				pointer := uint32(0)
				if value != "" {
					offset, exists := intern[value]
					if !exists {
						offset = stringBlock.Len()
						stringBlock.WriteString(value)
						stringBlock.WriteByte(0)
						intern[value] = offset
					}
					pointer = uint32(offset + len(spec.rows)*stride - rowStart - position)
				}
				for e := 0; e < elements(col); e++ {
					binary.LittleEndian.PutUint32(next32(&recordsData), pointer)
				}
			case 'f':
				for _, value := range floatCells(t, cell, elements(col), rowIndex, col.name) {
					binary.LittleEndian.PutUint32(next32(&recordsData), math.Float32bits(value))
				}
			case 'i':
				for _, value := range intCells(t, cell, elements(col), rowIndex, col.name) {
					putBits(&recordsData, uint64(value), col.bits, col.signed)
				}
			default:
				t.Fatalf("field %s: unknown kind %q", col.name, string(col.kind))
			}
			position += widths[index]
		}
		if recordsData.Len() != rowStart+stride {
			t.Fatalf("row %d wrote %d bytes, want stride %d", rowIndex, recordsData.Len()-rowStart, stride)
		}
	}
	relationBytes := 0
	if len(relations) > 0 {
		relationBytes = 12 + 8*len(relations)
	}
	metadataEnd := 72 + 40 + 4*len(spec.columns) + 24*len(inlineColumns)
	file := make([]byte, metadataEnd)
	copy(file, "WDC3")
	put := func(offset int, value uint32) { binary.LittleEndian.PutUint32(file[4+offset:8+offset], value) }
	put(0, uint32(len(spec.rows)))
	put(4, uint32(len(inlineColumns)))
	put(8, uint32(stride))
	put(16, spec.tableHash)
	put(20, spec.layoutHash)
	put(24, firstID)
	put(28, lastID)
	put(40, uint32(len(spec.columns)))
	put(52, uint32(24*len(inlineColumns)))
	put(64, 1)
	partition := 72
	binary.LittleEndian.PutUint32(file[partition+8:partition+12], uint32(metadataEnd))
	binary.LittleEndian.PutUint32(file[partition+12:partition+16], uint32(len(spec.rows)))
	binary.LittleEndian.PutUint32(file[partition+16:partition+20], uint32(stringBlock.Len()))
	identityBytes := 0
	if spec.identityExternal {
		identityBytes = 4 * len(spec.rows)
	}
	binary.LittleEndian.PutUint32(file[partition+24:partition+28], uint32(identityBytes))
	binary.LittleEndian.PutUint32(file[partition+28:partition+32], uint32(relationBytes))
	offset := 0
	for index := range spec.columns {
		entry := 72 + 40 + index*4
		binary.LittleEndian.PutUint16(file[entry+2:entry+4], uint16(offset))
		offset += widths[index]
	}
	for storageIndex, columnIndex := range inlineColumns {
		col := spec.columns[columnIndex]
		storage := 72 + 40 + 4*len(spec.columns) + storageIndex*24
		bitOffset := 0
		for _, prior := range inlineColumns {
			if prior == columnIndex {
				break
			}
			bitOffset += widths[prior] * 8
		}
		binary.LittleEndian.PutUint16(file[storage:storage+2], uint16(bitOffset))
		bits := uint16(col.bits)
		if col.kind != 'i' {
			bits = 32
		}
		binary.LittleEndian.PutUint16(file[storage+2:storage+4], bits*uint16(elements(col)))
	}
	file = append(file, recordsData.Bytes()...)
	file = append(file, stringBlock.Bytes()...)
	for _, id := range idList {
		file = binary.LittleEndian.AppendUint32(file, id)
	}
	if relationBytes > 0 {
		file = binary.LittleEndian.AppendUint32(file, uint32(len(relations)))
		file = binary.LittleEndian.AppendUint32(file, 0)
		file = binary.LittleEndian.AppendUint32(file, 0)
		for _, relation := range relations {
			file = binary.LittleEndian.AppendUint32(file, relation.foreign)
			file = binary.LittleEndian.AppendUint32(file, relation.ordinal)
		}
	}
	return file, buildDBD(t, spec, dbdLayout(spec))
}

func dbdLayout(spec fixtureTable) uint32 {
	if spec.dbdLayout != 0 {
		return spec.dbdLayout
	}
	return spec.layoutHash
}

func identityIndex(spec fixtureTable) int {
	for index, col := range spec.columns {
		if col.identity {
			return index
		}
	}
	return 0
}

func cellID(t *testing.T, cell any) uint32 {
	t.Helper()
	value := cellInt(t, cell)
	if value < 0 {
		t.Fatalf("negative identity %d", value)
	}
	return uint32(value)
}

func cellInt(t *testing.T, cell any) int64 {
	t.Helper()
	switch value := cell.(type) {
	case int64:
		return value
	case uint64:
		return int64(value)
	case int:
		return int64(value)
	case uint32:
		return int64(value)
	}
	t.Fatalf("integer cell required, got %T", cell)
	return 0
}

func intCells(t *testing.T, cell any, elements, rowIndex int, name string) []int64 {
	t.Helper()
	switch value := cell.(type) {
	case []int64:
		if len(value) != elements {
			t.Fatalf("row %d field %s: %d elements, want %d", rowIndex, name, len(value), elements)
		}
		return value
	default:
		if elements != 1 {
			t.Fatalf("row %d field %s: array cell required", rowIndex, name)
		}
		return []int64{cellInt(t, cell)}
	}
}

func floatCells(t *testing.T, cell any, elements, rowIndex int, name string) []float32 {
	t.Helper()
	switch value := cell.(type) {
	case []float32:
		if len(value) != elements {
			t.Fatalf("row %d field %s: %d elements, want %d", rowIndex, name, len(value), elements)
		}
		return value
	case float32:
		if elements != 1 {
			t.Fatalf("row %d field %s: array cell required", rowIndex, name)
		}
		return []float32{value}
	}
	t.Fatalf("row %d field %s: float cell required, got %T", rowIndex, name, cell)
	return nil
}

func next32(buffer *bytes.Buffer) []byte {
	start := buffer.Len()
	buffer.Write(make([]byte, 4))
	return buffer.Bytes()[start : start+4]
}

func putBits(buffer *bytes.Buffer, value uint64, bits int, signed bool) {
	if signed && bits < 64 {
		value = uint64(int64(value)<<(64-bits)) >> (64 - bits)
	}
	for i := 0; i < bits/8; i++ {
		buffer.WriteByte(byte(value >> (8 * i)))
	}
}

// buildDBD emits a strict DBD document whose variant binds by layout hash.
func buildDBD(t *testing.T, spec fixtureTable, layoutHash uint32) string {
	t.Helper()
	var columns, fields strings.Builder
	for _, col := range spec.columns {
		kind := "int"
		switch col.kind {
		case 'f':
			kind = "float"
		case 's':
			kind = "string"
			if col.loc {
				kind = "locstring"
			}
		}
		if col.foreign != "" {
			parts := strings.Split(col.foreign, "::")
			if len(parts) != 2 {
				t.Fatalf("bad foreign key %q", col.foreign)
			}
			kind = fmt.Sprintf("int<%s::%s>", parts[0], parts[1])
		}
		fmt.Fprintf(&columns, "%s %s\n", kind, col.name)
		switch {
		case col.relation:
			fmt.Fprintf(&fields, "$noninline,relation$%s\n", col.name)
		case col.identity && spec.identityExternal:
			fmt.Fprintf(&fields, "$noninline,id$%s\n", col.name)
		case col.identity:
			fmt.Fprintf(&fields, "$id$%s<u32>\n", col.name)
		default:
			suffix := "<32>"
			switch {
			case col.kind == 'f':
				suffix = "<32>"
			case col.kind == 's':
				suffix = ""
			case !col.signed:
				suffix = fmt.Sprintf("<u%d>", col.bits)
			case col.bits != 32:
				suffix = fmt.Sprintf("<%d>", col.bits)
			}
			array := ""
			if col.elements > 0 {
				array = fmt.Sprintf("[%d]", col.elements)
			}
			fmt.Fprintf(&fields, "%s%s%s\n", col.name, suffix, array)
		}
	}
	return fmt.Sprintf("COLUMNS\n%s\nBUILD 12.1.0.69875\nLAYOUT %08X\n%s", columns.String(), layoutHash, fields.String())
}

// newFixture stores every table in one synthetic offline CDN root and seeds
// the pinned definitions so preparation needs no network.
func newFixture(t *testing.T, tables []fixtureTable) *fixture {
	t.Helper()
	ctx := context.Background()
	workspace := filepath.Join(t.TempDir(), "workspace")
	if _, err := vault.Initialize(ctx, workspace); err != nil {
		t.Fatal(err)
	}
	files := map[uint32]testkit.FileObject{}
	dbds := map[string]string{}
	entries := []string{}
	fileIDs := map[string]uint32{}
	for index, spec := range tables {
		if spec.tableHash == 0 {
			spec.tableHash = 0xf00d0000 + uint32(index)
		}
		if spec.layoutHash == 0 {
			spec.layoutHash = 0xabcd0000 + uint32(index)
		}
		if _, exists := fileIDs[spec.name]; exists {
			t.Fatalf("duplicate fixture table %s", spec.name)
		}
		wdc, dbd := buildTable(t, spec)
		if spec.raw != nil {
			wdc = spec.raw
		}
		if spec.mutate != nil {
			wdc = spec.mutate(wdc)
		}
		if spec.object != nil {
			files[spec.fileDataID] = testkit.FileObject{Decoded: wdc, Object: spec.object}
		} else {
			files[spec.fileDataID] = testkit.FileObject{Decoded: wdc}
		}
		dbds[spec.name] = dbd
		fileIDs[spec.name] = spec.fileDataID
		entries = append(entries, fmt.Sprintf(`{"tableName":%q,"tableHash":"%08X","db2FileDataID":%d}`, spec.name, spec.tableHash, spec.fileDataID))
	}
	pin := testkit.CachedFiles(t, workspace, files)
	seedDefinitions(t, workspace, pin.Data.DefinitionCommit, "["+strings.Join(entries, ",")+"]", dbds)
	return &fixture{t: t, workspace: workspace, pin: pin, fileIDs: fileIDs}
}

// seedDefinitions pre-publishes the manifest and DBD documents exactly the way
// records.Definitions caches them, so Prepare works offline.
func seedDefinitions(t *testing.T, workspace, commit, manifest string, dbds map[string]string) {
	t.Helper()
	ctx := context.Background()
	store, err := vault.OpenStore(workspace)
	if err != nil {
		t.Fatal(err)
	}
	metadata, err := store.OpenMetadata(ctx)
	if err != nil {
		t.Fatal(err)
	}
	defer metadata.Close()
	base := "https://raw.githubusercontent.com/wowdev/WoWDBDefs/" + commit + "/"
	save := func(url, text string) {
		t.Helper()
		ref, err := store.PublishBlob(ctx, vault.BlobInput{Reader: strings.NewReader(text), MaxBytes: 4 << 20})
		if err != nil {
			t.Fatal(err)
		}
		document, err := json.Marshal(map[string]any{"url": url, "blob": ref})
		if err != nil {
			t.Fatal(err)
		}
		digest := sha256.Sum256([]byte(url))
		key := "definition/" + hex.EncodeToString(digest[:])
		if err := metadata.CommitDocuments(ctx, vault.Mutation{Key: key, Value: document}); err != nil {
			t.Fatal(err)
		}
	}
	save(base+"manifest.json", manifest)
	for name, text := range dbds {
		save(base+"definitions/"+name+".dbd", text)
	}
}
