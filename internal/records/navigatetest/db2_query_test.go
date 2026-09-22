package navigatetest

import (
	"context"
	"errors"
	"testing"

	"github.com/follenfang/lycheedev/internal/records"
	"github.com/follenfang/lycheedev/internal/records/table"
)

// sampleTable exercises the storage shapes used across tests: inline identity,
// localized and plain strings, signed/unsigned ints, an int array, a float and
// a foreign-key field.
func sampleTable(rows [][]any) fixtureTable {
	return fixtureTable{
		name: "Sample", fileDataID: 201,
		columns: []fixtureColumn{
			{name: "ID", kind: 'i', bits: 32, identity: true},
			{name: "Name_lang", kind: 's', loc: true},
			{name: "Note", kind: 's'},
			{name: "Value", kind: 'i', bits: 32, signed: true},
			{name: "Score", kind: 'i', bits: 8, signed: true},
			{name: "Rank", kind: 'i', bits: 32},
			{name: "Flags", kind: 'i', bits: 32, elements: 2},
			{name: "Ratio", kind: 'f'},
			{name: "SpellID", kind: 'i', bits: 32, foreign: "Spell::ID"},
		},
		rows: rows,
	}
}

// spellEffectTable mirrors the legacy SpellEffect shape used by the db2 golden
// oracles: external ID, non-inline relationship SpellID, plain fields.
func spellEffectTable(rows [][]any) fixtureTable {
	return fixtureTable{
		name: "SpellEffect", fileDataID: 202, identityExternal: true,
		columns: []fixtureColumn{
			{name: "ID", kind: 'i', bits: 32, identity: true},
			{name: "SpellID", kind: 'i', bits: 32, relation: true, foreign: "Spell::ID"},
			{name: "Effect", kind: 'i', bits: 32, signed: true},
			{name: "EffectTriggerSpell", kind: 'i', bits: 32},
			{name: "EffectMiscValue", kind: 'i', bits: 32, signed: true, elements: 2},
		},
		rows: rows,
	}
}

// spellNameTable mirrors the legacy SpellName golden shape (non-inline ID and
// one localized string) for the db2 schema vocabulary cross-check.
func spellNameTable(rows [][]any) fixtureTable {
	return fixtureTable{
		name: "SpellName", fileDataID: 203, identityExternal: true,
		columns: []fixtureColumn{
			{name: "ID", kind: 'i', bits: 32, identity: true},
			{name: "Name_lang", kind: 's', loc: true},
		},
		rows: rows,
	}
}

func sampleRows() [][]any {
	return [][]any{
		{int64(2923), "Rooting Attack", "note one", int64(-5), int64(-3), uint64(7), []int64{1, 2}, float32(1.25), uint64(100)},
		{int64(2927), "rooting ATTACK II", "", int64(10), int64(127), uint64(8), []int64{3, 4}, float32(-0.5), uint64(100)},
		{int64(2928), "Rooting Attack III", "third", int64(0), int64(0), uint64(9), []int64{5, 6}, float32(0.25), uint64(200)},
		{int64(3000), "Healing Wave", "fourth", int64(42), int64(7), uint64(8), []int64{7, 8}, float32(2), uint64(100)},
	}
}

func TestSchemaReportsFieldsTypesArraysAndKeys(t *testing.T) {
	fx := newFixture(t, []fixtureTable{sampleTable(sampleRows())})
	reading, err := records.InspectDataSchema(context.Background(), fx.workspace, fx.pin.ID, "Sample", fixtureQuery())
	if err != nil {
		t.Fatal(err)
	}
	result := reading.Result
	if result.Table != "Sample" || result.RowCount != 4 {
		t.Fatalf("identity/rowCount = %+v", result)
	}
	types := map[string]string{}
	for _, field := range result.Fields {
		types[field.Name] = field.Type
	}
	want := map[string]string{
		"ID": "dbFieldUInt32", "Name_lang": "dbFieldString", "Note": "dbFieldString",
		"Value": "dbFieldInt32", "Score": "dbFieldInt8", "Rank": "dbFieldUInt32",
		"Flags": "dbFieldUInt32", "Ratio": "dbFieldFloat", "SpellID": "dbFieldUInt32",
	}
	for name, expected := range want {
		if types[name] != expected {
			t.Fatalf("field %s type = %q, want %q (%v)", name, types[name], expected, types)
		}
	}
	if len(result.Keys) != 1 || result.Keys[0] != "ID" {
		t.Fatalf("keys = %v", result.Keys)
	}
	if len(result.RelationshipFields) != 1 || result.RelationshipFields[0] != "SpellID" {
		t.Fatalf("relationship fields = %v", result.RelationshipFields)
	}
	if len(result.ArrayFields) != 1 || result.ArrayFields[0] != "Flags" {
		t.Fatalf("array fields = %v", result.ArrayFields)
	}
	for _, field := range result.Fields {
		switch field.Name {
		case "Flags":
			if !field.Array || field.Elements != 2 {
				t.Fatalf("array shape = %+v", field)
			}
		case "SpellID":
			if field.ForeignTable != "Spell" || field.ForeignColumn != "ID" {
				t.Fatalf("foreign key = %+v", field)
			}
		case "Score":
			if !field.Signed || field.Bits != 8 {
				t.Fatalf("signed shape = %+v", field)
			}
		}
	}
	// Fixed references survive in every result (DAT-03).
	flags := result.DataContext
	if flags.Snapshot != fx.pin.ID || flags.Pin != *fx.pin.Data {
		t.Fatalf("fixed references lost: %+v", flags)
	}
	if flags.Source != "cdn" || !flags.Complete || flags.Truncated || flags.TruncationReason != "none" {
		t.Fatalf("honesty flags = %+v", flags)
	}
	if len(flags.Tables) != 1 || flags.Tables[0].Name != "Sample" || flags.Tables[0].LogicalRows != 4 {
		t.Fatalf("table provenance = %+v", flags.Tables)
	}
	if flags.Tables[0].DefinitionSHA256 == "" || flags.Tables[0].ContentKey == "" || flags.Tables[0].LayoutHash == "" {
		t.Fatalf("table identity incomplete: %+v", flags.Tables[0])
	}
	if len(reading.Captures) != 1 || reading.Captures[0].ID == "" {
		t.Fatalf("capture missing: %+v", reading.Captures)
	}
}

// Cross-check against the legacy schema oracle
// (wowdata fixtures/golden/go/db2/schema-spellname-classic-era.json): the type
// vocabulary must reproduce {"ID": "dbFieldNonInlineID", "Name_lang": "dbFieldString"}.
func TestSchemaMatchesLegacyTypeVocabulary(t *testing.T) {
	fx := newFixture(t, []fixtureTable{spellNameTable([][]any{
		{int64(1), "Word of Recall (OLD)"},
		{int64(2), "Attack"},
	})})
	reading, err := records.InspectDataSchema(context.Background(), fx.workspace, fx.pin.ID, "SpellName", fixtureQuery())
	if err != nil {
		t.Fatal(err)
	}
	types := map[string]string{}
	for _, field := range reading.Result.Fields {
		types[field.Name] = field.Type
	}
	if types["ID"] != "dbFieldNonInlineID" || types["Name_lang"] != "dbFieldString" {
		t.Fatalf("legacy type vocabulary broken: %v", types)
	}
	if len(types) != 2 {
		t.Fatalf("unexpected fields: %v", types)
	}
}

// Cross-check against the legacy search oracle
// (wowdata fixtures/golden/go/db2/search-spellname-attack-classic-era.json):
// case-insensitive substring over one field, ascending record order, first
// `limit` matches as complete rows.
func TestSearchIsCaseInsensitiveOrderedAndBounded(t *testing.T) {
	fx := newFixture(t, []fixtureTable{sampleTable(sampleRows())})
	complete, err := records.InspectDataSearch(context.Background(), fx.workspace, fx.pin.ID, "Sample", fixtureQuery(),
		records.DB2SearchRequest{Field: "Name_lang", Query: "Attack", Limit: 3})
	if err != nil {
		t.Fatal(err)
	}
	if complete.Result.Count != 3 || complete.Result.Mode != "search" {
		t.Fatalf("result = %+v", complete.Result)
	}
	wantIDs := []any{uint64(2923), uint64(2927), uint64(2928)}
	for index, row := range complete.Result.Rows {
		if row["ID"] != wantIDs[index] {
			t.Fatalf("row %d id = %v", index, row["ID"])
		}
	}
	if len(complete.Result.Rows[0]) != 9 {
		t.Fatalf("search rows carry all fields: %v", complete.Result.Rows[0])
	}
	if !complete.Result.Complete || complete.Result.Truncated {
		t.Fatalf("exact fit must be complete: %+v", complete.Result.DataContext)
	}
	bounded, err := records.InspectDataSearch(context.Background(), fx.workspace, fx.pin.ID, "Sample", fixtureQuery(),
		records.DB2SearchRequest{Field: "Name_lang", Query: "Attack", Limit: 2})
	if err != nil {
		t.Fatal(err)
	}
	if bounded.Result.Count != 2 || !bounded.Result.Truncated || bounded.Result.Complete {
		t.Fatalf("bounded search = %+v", bounded.Result)
	}
	if bounded.Result.TruncationReason != "limit_reached" {
		t.Fatalf("truncation reason = %q", bounded.Result.TruncationReason)
	}
}

// Cross-check against the legacy foreign-key oracle
// (wowdata fixtures/golden/go/db2/foreign-key-spelleffect-spell-1-classic-era.json),
// with the intentional change that limit 0 is rejected instead of returning an
// unbounded set.
func TestForeignKeyBoundsAndSemantics(t *testing.T) {
	fx := newFixture(t, []fixtureTable{spellEffectTable([][]any{
		{int64(35454), int64(43680), int64(6), uint64(43681), []int64{0, 0}},
		{int64(678946), int64(513), int64(28), uint64(0), []int64{329, 67}},
		{int64(692845), int64(1), int64(252), uint64(0), []int64{0, 0}},
	})})
	multi := newFixture(t, []fixtureTable{sampleTable(sampleRows())})
	reading, err := records.InspectDataForeignKey(context.Background(), fx.workspace, fx.pin.ID, "SpellEffect", fixtureQuery(),
		records.DB2ForeignKeyRequest{Field: "SpellID", Value: 1, Limit: 10})
	if err != nil {
		t.Fatal(err)
	}
	if reading.Result.Count != 1 || reading.Result.Mode != "foreign-key" {
		t.Fatalf("result = %+v", reading.Result)
	}
	row := reading.Result.Rows[0]
	if row["ID"] != uint64(692845) || row["SpellID"] != uint64(1) || row["Effect"] != int64(252) {
		t.Fatalf("row = %v", row)
	}
	if misc, ok := row["EffectMiscValue"].([]any); !ok || len(misc) != 2 || misc[0] != int64(0) {
		t.Fatalf("array values = %v", row["EffectMiscValue"])
	}
	if !reading.Result.Complete || reading.Result.Truncated {
		t.Fatalf("flags = %+v", reading.Result.DataContext)
	}
	// The relationship value itself is addressable as a foreign key.
	viaRelation, err := records.InspectDataForeignKey(context.Background(), fx.workspace, fx.pin.ID, "SpellEffect", fixtureQuery(),
		records.DB2ForeignKeyRequest{Field: "SpellID", Value: 513, Limit: 10})
	if err != nil {
		t.Fatal(err)
	}
	if viaRelation.Result.Count != 1 || viaRelation.Result.Rows[0]["ID"] != uint64(678946) {
		t.Fatalf("relation lookup = %+v", viaRelation.Result)
	}
	// Missing relationships are nil, so a value-0 request matches nothing.
	bounded, err := records.InspectDataForeignKey(context.Background(), fx.workspace, fx.pin.ID, "SpellEffect", fixtureQuery(),
		records.DB2ForeignKeyRequest{Field: "SpellID", Value: 0, Limit: 10})
	if err != nil {
		t.Fatal(err)
	}
	if bounded.Result.Count != 0 || !bounded.Result.Complete {
		t.Fatalf("zero match = %+v", bounded.Result)
	}
	if _, err := records.InspectDataForeignKey(context.Background(), fx.workspace, fx.pin.ID, "SpellEffect", fixtureQuery(),
		records.DB2ForeignKeyRequest{Field: "SpellID", Value: 1, Limit: 0}); !errors.Is(err, records.ErrLimitRequired) {
		t.Fatalf("limit 0 must be rejected, got %v", err)
	}
	if _, err := records.InspectDataForeignKey(context.Background(), fx.workspace, fx.pin.ID, "SpellEffect", fixtureQuery(),
		records.DB2ForeignKeyRequest{Field: "SpellID", Value: 1, Limit: records.MaxForeignKeyRows + 1}); !errors.Is(err, records.ErrQueryLimit) {
		t.Fatalf("oversized limit must be rejected, got %v", err)
	}
	if _, err := records.InspectDataForeignKey(context.Background(), multi.workspace, multi.pin.ID, "Sample", fixtureQuery(),
		records.DB2ForeignKeyRequest{Field: "Name_lang", Value: 1, Limit: 10}); !errors.Is(err, records.ErrFieldType) {
		t.Fatalf("string foreign key must be rejected, got %v", err)
	}
	if _, err := records.InspectDataForeignKey(context.Background(), multi.workspace, multi.pin.ID, "Sample", fixtureQuery(),
		records.DB2ForeignKeyRequest{Field: "Missing", Value: 1, Limit: 10}); !errors.Is(err, records.ErrFieldUnknown) {
		t.Fatalf("unknown field must be rejected, got %v", err)
	}
	// Truncation probing: three matches with limit 2 is honestly truncated.
	cut, err := records.InspectDataForeignKey(context.Background(), multi.workspace, multi.pin.ID, "Sample", fixtureQuery(),
		records.DB2ForeignKeyRequest{Field: "SpellID", Value: 100, Limit: 2})
	if err != nil {
		t.Fatal(err)
	}
	if cut.Result.Count != 2 || !cut.Result.Truncated || cut.Result.Complete || cut.Result.TruncationReason != "limit_reached" {
		t.Fatalf("cut foreign key = %+v", cut.Result)
	}
}

func collectFrames(t *testing.T, fx *fixture, request records.DB2StreamRequest) ([]records.StreamFrame, error) {
	t.Helper()
	frames := []records.StreamFrame{}
	_, err := records.StreamDataRows(context.Background(), fx.workspace, fx.pin.ID, "Sample", fixtureQuery(), request,
		func(frame records.StreamFrame) error {
			frames = append(frames, frame)
			return nil
		})
	return frames, err
}

func TestStreamEmitsTypedFrameContract(t *testing.T) {
	fx := newFixture(t, []fixtureTable{sampleTable(sampleRows())})
	frames, err := collectFrames(t, fx, records.DB2StreamRequest{Fields: []string{"ID", "Name_lang"}, Filter: "Rank=8", Limit: 10})
	if err != nil {
		t.Fatal(err)
	}
	if len(frames) != 4 || frames[0].Frame != "begin" || frames[len(frames)-1].Frame != "end" {
		t.Fatalf("frames = %+v", frames)
	}
	begin := frames[0].Begin
	if begin.Snapshot != fx.pin.ID || begin.Table != "Sample" || begin.Limit != 10 || begin.Filter != "Rank=8" {
		t.Fatalf("begin = %+v", begin)
	}
	if begin.Identity.Name != "Sample" || begin.LayoutHash == "" {
		t.Fatalf("begin identity = %+v", begin.Identity)
	}
	for index, frame := range frames[1:3] {
		if frame.Frame != "record" || frame.Record.Index != index {
			t.Fatalf("record frame = %+v", frame.Record)
		}
		if len(frame.Record.Row) != 2 {
			t.Fatalf("projection leaked: %v", frame.Record.Row)
		}
		if frame.Record.RecordID == 0 {
			t.Fatalf("record id missing: %+v", frame.Record)
		}
	}
	end := frames[3].End
	if !end.Complete || end.Truncated || end.Count != 2 || end.TruncationReason != "none" {
		t.Fatalf("end = %+v", end)
	}
	// The limit cut is reported in the end frame, never hidden.
	cut, err := collectFrames(t, fx, records.DB2StreamRequest{Limit: 1})
	if err != nil {
		t.Fatal(err)
	}
	last := cut[len(cut)-1]
	if last.Frame != "end" || last.End.Count != 1 || !last.End.Truncated || last.End.Complete || last.End.TruncationReason != "limit_reached" {
		t.Fatalf("cut end = %+v", last.End)
	}
}

func TestStreamErrorFrameEndsStreamWithoutEnd(t *testing.T) {
	corrupt := sampleTable(sampleRows())
	corrupt.mutate = func(raw []byte) []byte {
		out := append([]byte(nil), raw...)
		// Break the first row's Name_lang string pointer so decoding fails
		// with a bounded format error instead of inventing values.
		metadataEnd := 72 + 40 + 4*9 + 24*9
		for i := metadataEnd + 4; i < metadataEnd+8; i++ {
			out[i] = 0xff
		}
		return out
	}
	fx := newFixture(t, []fixtureTable{corrupt})
	frames := []records.StreamFrame{}
	_, err := records.StreamDataRows(context.Background(), fx.workspace, fx.pin.ID, "Sample", fixtureQuery(),
		records.DB2StreamRequest{Limit: 10}, func(frame records.StreamFrame) error {
			frames = append(frames, frame)
			return nil
		})
	if err == nil {
		t.Fatalf("corrupt rows must fail, got frames %+v", frames)
	}
	last := frames[len(frames)-1]
	if last.Frame != "error" || last.Error == nil || last.Error.Code == "" {
		t.Fatalf("error frame = %+v", last)
	}
	for _, frame := range frames {
		if frame.Frame == "end" {
			t.Fatal("a failed stream must not emit an end frame")
		}
	}
}

func TestStreamCancellationStopsReads(t *testing.T) {
	fx := newFixture(t, []fixtureTable{sampleTable(sampleRows())})
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	frames := []records.StreamFrame{}
	_, err := records.StreamDataRows(ctx, fx.workspace, fx.pin.ID, "Sample", fixtureQuery(),
		records.DB2StreamRequest{Limit: 10}, func(frame records.StreamFrame) error {
			frames = append(frames, frame)
			if frame.Frame == "record" {
				cancel()
			}
			return nil
		})
	if !errors.Is(err, context.Canceled) {
		t.Fatalf("cancellation must surface, got %v", err)
	}
	emitted := 0
	for _, frame := range frames {
		if frame.Frame == "record" {
			emitted++
		}
	}
	if emitted != 1 {
		t.Fatalf("cancellation kept reading: %d record frames", emitted)
	}
}

func TestModesRejectUnknownSchemaCorruptAndEncrypted(t *testing.T) {
	t.Run("unknown-schema", func(t *testing.T) {
		unknown := sampleTable(sampleRows()[:1])
		unknown.dbdLayout = 0xdeadbeef
		fx := newFixture(t, []fixtureTable{unknown})
		_, err := records.InspectDataSchema(context.Background(), fx.workspace, fx.pin.ID, "Sample", fixtureQuery())
		if err == nil {
			t.Fatal("an unbound layout must fail, not return a zero-value schema")
		}
	})
	t.Run("truncated", func(t *testing.T) {
		cut := sampleTable(sampleRows()[:1])
		cut.mutate = func(raw []byte) []byte { return raw[:len(raw)-8] }
		fx := newFixture(t, []fixtureTable{cut})
		_, err := records.InspectDataSchema(context.Background(), fx.workspace, fx.pin.ID, "Sample", fixtureQuery())
		if err == nil {
			t.Fatal("a truncated file must fail")
		}
	})
	t.Run("corrupt-rows", func(t *testing.T) {
		corrupt := sampleTable(sampleRows())
		corrupt.mutate = func(raw []byte) []byte {
			out := append([]byte(nil), raw...)
			metadataEnd := 72 + 40 + 4*9 + 24*9
			for i := metadataEnd + 4; i < metadataEnd+8; i++ {
				out[i] = 0xff
			}
			return out
		}
		fx := newFixture(t, []fixtureTable{corrupt})
		reading, err := records.InspectDataSearch(context.Background(), fx.workspace, fx.pin.ID, "Sample", fixtureQuery(),
			records.DB2SearchRequest{Field: "Name_lang", Query: "", Limit: 10})
		if err == nil {
			t.Fatalf("corrupt rows must fail, not silently return values: %+v", reading.Result)
		}
	})
	t.Run("oversized-field", func(t *testing.T) {
		huge := sampleTable([][]any{{int64(1), longText(1<<20 + 128), "", int64(0), int64(0), uint64(0), []int64{0, 0}, float32(0), uint64(0)}})
		fx := newFixture(t, []fixtureTable{huge})
		_, err := records.InspectDataSearch(context.Background(), fx.workspace, fx.pin.ID, "Sample", fixtureQuery(),
			records.DB2SearchRequest{Field: "Name_lang", Query: "", Limit: 10})
		if !errors.Is(err, table.ErrLimit) {
			t.Fatalf("oversized field must hit the text budget, got %v", err)
		}
	})
	t.Run("encrypted", func(t *testing.T) {
		secret := sampleTable(sampleRows()[:1])
		// BLTE with one 'E' (encrypted) section and no keys available.
		payload := append([]byte{8, 1, 0, 0, 0, 0, 0, 0, 0, 4, 0, 0, 0, 0, 'S'}, []byte("ciphertext")...)
		block := append([]byte{'E'}, payload...)
		object := make([]byte, 8+len(block))
		copy(object, "BLTE")
		object[4], object[5], object[6], object[7] = 0, 0, 0, 0
		copy(object[8:], block)
		secret.object = object
		fx := newFixture(t, []fixtureTable{secret})
		_, err := records.InspectDataSchema(context.Background(), fx.workspace, fx.pin.ID, "Sample", fixtureQuery())
		if err == nil {
			t.Fatal("encrypted content without keys must fail, not decode to zeros")
		}
	})
	t.Run("cancelled", func(t *testing.T) {
		fx := newFixture(t, []fixtureTable{sampleTable(sampleRows()[:1])})
		ctx, cancel := context.WithCancel(context.Background())
		cancel()
		if _, err := records.InspectDataSchema(ctx, fx.workspace, fx.pin.ID, "Sample", fixtureQuery()); !errors.Is(err, context.Canceled) {
			t.Fatalf("cancelled context must surface, got %v", err)
		}
	})
}

func longText(size int) string {
	out := make([]byte, size)
	for i := range out {
		out[i] = 'a' + byte(i%26)
	}
	return string(out)
}
