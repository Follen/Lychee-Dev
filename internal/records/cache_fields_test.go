package records_test

import (
	"bytes"
	"context"
	"encoding/binary"
	"encoding/json"
	"errors"
	"math"
	"reflect"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/records"
	"github.com/follenfang/lycheedev/internal/records/schema"
)

func cacheDefinition(t *testing.T, text string) schema.Definition {
	t.Helper()
	doc, err := schema.Parse(context.Background(), []byte(text))
	if err != nil {
		t.Fatal(err)
	}
	definition, err := doc.Select(context.Background(), "12.1.0.69875", "")
	if err != nil {
		t.Fatal(err)
	}
	return definition
}

func TestDecodeHotfixFields(t *testing.T) {
	definition := cacheDefinition(t, "COLUMNS\nint ID\nstring Name\nlocstring Label\nint Small\nint Wide\nfloat Rate\nint Parent\nint Values\n\nBUILD 12.1.0.69875\n$noninline,id$ID<32>\nName\nLabel\nSmall<8>\nWide<u64>\nRate\n$noninline,relation$Parent<u16>\nValues<u16>[2]\n")
	var payload bytes.Buffer
	payload.WriteString("Name\x00中文\x00")
	payload.WriteByte(0xfe)
	for _, value := range []any{uint64(math.MaxUint64), float32(1.5), uint16(0x1234), uint16(0), uint16(math.MaxUint16)} {
		if err := binary.Write(&payload, binary.LittleEndian, value); err != nil {
			t.Fatal(err)
		}
	}
	row, err := records.DecodeHotfixFields(context.Background(), payload.Bytes(), 42, definition)
	if err != nil {
		t.Fatal(err)
	}
	want := map[string]any{"ID": int64(42), "Name": "Name", "Label": "中文", "Small": int64(-2), "Wide": uint64(math.MaxUint64), "Rate": float64(1.5), "Parent": uint64(0x1234), "Values": []any{uint64(0), uint64(math.MaxUint16)}}
	if !reflect.DeepEqual(row, want) {
		t.Fatalf("got %#v want %#v", row, want)
	}
	encoded, err := json.Marshal(row)
	if err != nil || !bytes.Contains(encoded, []byte(`"Wide":18446744073709551615`)) {
		t.Fatal(string(encoded), err)
	}
	if _, err := records.DecodeHotfixFields(context.Background(), append(bytes.Clone(payload.Bytes()), 0), 42, definition); !errors.Is(err, records.ErrCacheFields) {
		t.Fatalf("trailing byte accepted: %v", err)
	}
}

func TestDecodeHotfixInlineAndHeaderIdentity(t *testing.T) {
	d := cacheDefinition(t, "COLUMNS\nint ID\n\nBUILD 12.1.0.69875\n$id$ID<u32>\n")
	raw := []byte{42, 0, 0, 0}
	row, err := records.DecodeHotfixFields(context.Background(), raw, 42, d)
	if err != nil || row["ID"] != uint64(42) {
		t.Fatal(row, err)
	}
	if _, err := records.DecodeHotfixFields(context.Background(), raw, 43, d); !errors.Is(err, records.ErrCacheFields) {
		t.Fatal(err)
	}
	d = cacheDefinition(t, "COLUMNS\nint ID\n\nBUILD 12.1.0.69875\n$noninline,id$ID<8>\n")
	row, err = records.DecodeHotfixFields(context.Background(), nil, math.MaxUint32, d)
	if err != nil || row["ID"] != int64(-1) {
		t.Fatal(row, err)
	}
	if _, err := records.DecodeHotfixFields(context.Background(), nil, 128, d); !errors.Is(err, records.ErrCacheFields) {
		t.Fatal(err)
	}
}

func TestDecodeHotfixRejectsMalformedPayloads(t *testing.T) {
	text := cacheDefinition(t, "COLUMNS\nstring Text\n\nBUILD 12.1.0.69875\nText\n")
	for _, raw := range [][]byte{nil, []byte("missing terminator"), {0xff, 0}, {'a', 0, 0}, {1, 0, 0, 0, 'a'}} {
		row, err := records.DecodeHotfixFields(context.Background(), raw, 0, text)
		if !errors.Is(err, records.ErrCacheFields) || row != nil {
			t.Fatal(raw, row, err)
		}
	}
	float := cacheDefinition(t, "COLUMNS\nfloat Value\n\nBUILD 12.1.0.69875\nValue\n")
	for _, bits := range []uint32{0x7f800000, 0xff800000, 0x7fc00000} {
		raw := make([]byte, 4)
		binary.LittleEndian.PutUint32(raw, bits)
		if _, err := records.DecodeHotfixFields(context.Background(), raw, 0, float); !errors.Is(err, records.ErrCacheFields) {
			t.Fatal(err)
		}
	}
	if _, err := records.DecodeHotfixFields(context.Background(), []byte{1}, 0, float); !errors.Is(err, records.ErrCacheFields) {
		t.Fatal(err)
	}
	array := cacheDefinition(t, "COLUMNS\nstring Names\n\nBUILD 12.1.0.69875\nNames[2]\n")
	row, err := records.DecodeHotfixFields(context.Background(), []byte("\x00B\x00"), 0, array)
	if err != nil || !reflect.DeepEqual(row["Names"], []any{"", "B"}) {
		t.Fatal(row, err)
	}
}

func TestDecodeHotfixBudgetsAndCancellation(t *testing.T) {
	d := cacheDefinition(t, "COLUMNS\nstring Text\n\nBUILD 12.1.0.69875\nText\n")
	raw := []byte(strings.Repeat("a", 1<<20) + "\x00")
	if _, err := records.DecodeHotfixFields(context.Background(), raw, 0, d); err != nil {
		t.Fatal(err)
	}
	if _, err := records.DecodeHotfixFields(context.Background(), append([]byte{'a'}, raw...), 0, d); !errors.Is(err, records.ErrCacheLimit) {
		t.Fatal(err)
	}
	canceled, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := records.DecodeHotfixFields(canceled, nil, 0, d); !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
	d.Fields[0].Array = true
	d.Fields[0].Elements = 65537
	if _, err := records.DecodeHotfixFields(context.Background(), nil, 0, d); !errors.Is(err, records.ErrCacheLimit) {
		t.Fatal(err)
	}
}

func FuzzDecodeHotfixFields(f *testing.F) {
	f.Add([]byte("ok\x00\x01\x00"))
	f.Add([]byte{0xff, 0})
	doc, err := schema.Parse(context.Background(), []byte("COLUMNS\nstring Name\nint Value\n\nBUILD 12.1.0.69875\nName\nValue<16>\n"))
	if err != nil {
		f.Fatal(err)
	}
	d, err := doc.Select(context.Background(), "12.1.0.69875", "")
	if err != nil {
		f.Fatal(err)
	}
	f.Fuzz(func(t *testing.T, raw []byte) {
		if len(raw) > 2<<20 {
			return
		}
		row, err := records.DecodeHotfixFields(context.Background(), raw, 0, d)
		if err == nil {
			if _, err := json.Marshal(row); err != nil {
				t.Fatal(err)
			}
		} else if row != nil {
			t.Fatal("partial fields on failure")
		}
	})
}
