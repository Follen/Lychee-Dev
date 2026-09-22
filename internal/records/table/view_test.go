package table_test

import (
	"bytes"
	"context"
	"encoding/binary"
	"errors"
	"math"
	"reflect"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/records/schema"
	"github.com/follenfang/lycheedev/internal/records/table"
)

func definition(t *testing.T, columns, fields string) *schema.Document {
	t.Helper()
	doc, err := schema.Parse(context.Background(), []byte("COLUMNS\n"+columns+"\n\nBUILD 12.1.0.69875\nLAYOUT 12345678\n"+fields+"\n"))
	if err != nil {
		t.Fatal(err)
	}
	return doc
}

func bind(t *testing.T, raw []byte, doc *schema.Document) *table.View {
	t.Helper()
	records, err := table.OpenRecords(context.Background(), bytes.NewReader(raw), int64(len(raw)), limits)
	if err != nil {
		t.Fatal(err)
	}
	v, err := records.Bind(context.Background(), doc, "12.1.0.69875")
	if err != nil {
		t.Fatal(err)
	}
	return v
}

func TestBoundFixedRowTypesIdentityAndRelationships(t *testing.T) {
	raw := recordFixture(true, words(99, 10), words(1, 0, 0, 0, 0))
	doc := definition(t, "int ID\nint Value\nint Parent", "$noninline,id$ID\nValue<32>\n$noninline,relation$Parent")
	v := bind(t, raw, doc)
	for _, id := range []uint32{10, 99} {
		got, err := v.Row(context.Background(), id, 100)
		want := map[string]any{"ID": uint64(id), "Value": int64(1), "Parent": uint64(0)}
		if err != nil || !reflect.DeepEqual(got, want) {
			t.Fatalf("%v %v", got, err)
		}
	}
	got, err := v.Row(context.Background(), 20, 100)
	if err != nil || got["Parent"] != nil {
		t.Fatalf("missing relationship: %v %v", got, err)
	}
	identity := v.Definition()
	identity.Fields[0].Name = "changed"
	if v.Definition().Fields[0].Name != "ID" || v.Definition().SHA256 == "" || v.Definition().Layout != "12345678" {
		t.Fatal("definition identity/ownership lost")
	}
	got["Value"] = int64(999)
	again, err := v.Row(context.Background(), 20, 100)
	if err != nil || again["Value"] != int64(2) {
		t.Fatal("row output alias")
	}
}

func typedSparseFixture() []byte {
	const start = 224
	base, _, _ := fixture(3)
	raw := make([]byte, start)
	copy(raw[:112], base[:112])
	binary.LittleEndian.PutUint32(raw[4:8], 1)
	binary.LittleEndian.PutUint32(raw[8:12], 4)
	binary.LittleEndian.PutUint32(raw[28:32], 10)
	binary.LittleEndian.PutUint32(raw[32:36], 10)
	binary.LittleEndian.PutUint16(raw[40:42], 1)
	binary.LittleEndian.PutUint32(raw[44:48], 4)
	binary.LittleEndian.PutUint32(raw[56:60], 96)
	binary.LittleEndian.PutUint32(raw[80:84], start)
	binary.LittleEndian.PutUint32(raw[84:88], 1)
	binary.LittleEndian.PutUint32(raw[100:104], 20)
	binary.LittleEndian.PutUint32(raw[104:108], 1)
	binary.LittleEndian.PutUint32(raw[108:112], 1)
	for i, width := range []uint16{32, 8, 32, 32} {
		binary.LittleEndian.PutUint16(raw[128+i*24+2:128+i*24+4], width)
	}
	data := append([]byte("猫\x00"), 249)
	data = append(data, words(math.Float32bits(1.25))...)
	data = binary.LittleEndian.AppendUint16(data, 3)
	data = binary.LittleEndian.AppendUint16(data, 65535)
	raw = append(raw, data...)
	binary.LittleEndian.PutUint32(raw[92:96], uint32(len(raw)))
	raw = append(raw, words(99, 10)...)
	raw = binary.LittleEndian.AppendUint32(raw, start)
	raw = binary.LittleEndian.AppendUint16(raw, uint16(len(data)))
	raw = append(raw, words(1, 0, 0, 0, 0)...)
	return append(raw, words(10)...)
}

func TestBoundSparseOrderedFields(t *testing.T) {
	raw := typedSparseFixture()
	doc := definition(t, "int ID\nstring Name\nint Score\nfloat Ratio\nint Flags\nint Parent", "$noninline,id$ID\nName\nScore<8>\nRatio\nFlags<u16>[2]\n$noninline,relation$Parent")
	v := bind(t, raw, doc)
	for _, id := range []uint32{10, 99} {
		got, err := v.Row(context.Background(), id, 3)
		want := map[string]any{"ID": uint64(id), "Name": "猫", "Score": int64(-7), "Ratio": float32(1.25), "Flags": []any{uint64(3), uint64(65535)}, "Parent": uint64(0)}
		if err != nil || !reflect.DeepEqual(got, want) {
			t.Fatalf("%v %v", got, err)
		}
	}
	if got, err := v.Row(context.Background(), 10, 2); !errors.Is(err, table.ErrLimit) || got != nil {
		t.Fatalf("%v %v", got, err)
	}
	bad := bytes.Clone(raw)
	for i := 224; i < 237; i++ {
		bad[i] = 0xff
	}
	broken := bind(t, bad, doc)
	if got, err := broken.Row(context.Background(), 10, 100); err == nil || got != nil {
		t.Fatal("bad sparse row returned partially decoded fields")
	}
}

func TestBoundRowsRejectWrongSchemaAndNonfiniteFloat(t *testing.T) {
	raw := recordFixture(true, nil, nil)
	records, err := table.OpenRecords(context.Background(), bytes.NewReader(raw), int64(len(raw)), limits)
	if err != nil {
		t.Fatal(err)
	}
	for _, fields := range []string{"$id$ID<u32>\nValue<32>", "$noninline,id$ID", "$noninline,id$ID\nValue<32>\nExtra<32>"} {
		doc := definition(t, "int ID\nint Value\nint Extra", fields)
		if _, err := records.Bind(context.Background(), doc, "12.1.0.69875"); !errors.Is(err, table.ErrFormat) {
			t.Fatal(err)
		}
	}
	wrong, err := schema.Parse(context.Background(), []byte("COLUMNS\nint ID\nint Value\n\nBUILD 12.1.0.69875\nLAYOUT DEADBEEF\n$noninline,id$ID\nValue<32>\n"))
	if err != nil {
		t.Fatal(err)
	}
	if _, err := records.Bind(context.Background(), wrong, "12.1.0.69875"); !errors.Is(err, schema.ErrMissing) {
		t.Fatal(err)
	}
	_, _, start := fixture(3)
	binary.LittleEndian.PutUint32(raw[start:start+4], math.Float32bits(float32(math.Inf(1))))
	v := bind(t, raw, definition(t, "int ID\nfloat Value", "$noninline,id$ID\nValue"))
	if got, err := v.Row(context.Background(), 10, 10); !errors.Is(err, table.ErrFormat) || got != nil {
		t.Fatalf("%v %v", got, err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := v.Row(ctx, 10, 10); !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
}

func TestBoundIntegersPreserve64Bits(t *testing.T) {
	raw, _, start := fixture(3)
	binary.LittleEndian.PutUint32(raw[12:16], 8)
	binary.LittleEndian.PutUint32(raw[96:100], 8)
	binary.LittleEndian.PutUint16(raw[start-22:start-20], 64)
	raw = append(raw[:start], bytes.Repeat([]byte{0xff}, 16)...)
	raw = append(raw, words(10, 20)...)
	for _, signed := range []bool{false, true} {
		size := "u64"
		if signed {
			size = "64"
		}
		v := bind(t, raw, definition(t, "int ID\nint Value", "$noninline,id$ID\nValue<"+size+">"))
		got, err := v.Row(context.Background(), 10, 10)
		if err != nil {
			t.Fatal(err)
		}
		if signed {
			if got["Value"] != int64(-1) {
				t.Fatal(got)
			}
		} else if got["Value"] != ^uint64(0) {
			t.Fatal(got)
		}
	}
}

func TestBoundFixedStringsAndStorageWidthMismatch(t *testing.T) {
	raw := stringFixture(3, []byte("hello\x00中文\x00"), 8, 10)
	v := bind(t, raw, definition(t, "int ID\nstring Name", "$noninline,id$ID\nName"))
	got, err := v.Row(context.Background(), 2, 6)
	if err != nil || got["Name"] != "中文" || got["ID"] != uint64(2) {
		t.Fatalf("%v %v", got, err)
	}
	r, err := table.OpenRecords(context.Background(), bytes.NewReader(raw), int64(len(raw)), limits)
	if err != nil {
		t.Fatal(err)
	}
	bad := definition(t, "int ID\nint Value", "$noninline,id$ID\nValue<u64>")
	if _, err := r.Bind(context.Background(), bad, "12.1.0.69875"); !errors.Is(err, table.ErrFormat) {
		t.Fatal("accepted 64-bit schema over 32-bit direct storage")
	}
}

func FuzzBoundSparseRow(f *testing.F) {
	f.Add([]byte("猫\x00"))
	f.Fuzz(func(t *testing.T, mutation []byte) {
		if len(mutation) > 13 {
			t.Skip()
		}
		raw := typedSparseFixture()
		copy(raw[224:237], mutation)
		doc := definition(t, "int ID\nstring Name\nint Score\nfloat Ratio\nint Flags\nint Parent", strings.Join([]string{"$noninline,id$ID", "Name", "Score<8>", "Ratio", "Flags<u16>[2]", "$noninline,relation$Parent"}, "\n"))
		v := bind(t, raw, doc)
		_, _ = v.Row(context.Background(), 10, 32)
	})
}
