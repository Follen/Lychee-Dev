package table_test

import (
	"bytes"
	"context"
	"encoding/binary"
	"github.com/follenfang/lycheedev/internal/records/table"
	"testing"
)

func TestCopyCommonUsesLogicalID(t *testing.T) {
	for _, tc := range []struct {
		name      string
		overrides []byte
		want      uint64
	}{
		{"both", words(7, 99, 9, 77), 77},
		{"origin-only", words(7, 99), 33},
		{"default", nil, 33},
	} {
		t.Run(tc.name, func(t *testing.T) {
			raw := columnFixture(2, 0, 0, [3]uint32{33}, tc.overrides)
			binary.LittleEndian.PutUint16(raw[40:], 4)
			binary.LittleEndian.PutUint32(raw[28:], 7)
			binary.LittleEndian.PutUint32(raw[32:], 9)
			binary.LittleEndian.PutUint32(raw[96:], 4)
			binary.LittleEndian.PutUint32(raw[108:], 1)
			raw = append(raw, words(7, 9, 7)...)
			ctx := context.Background()
			r, err := table.OpenRecords(ctx, bytes.NewReader(raw), int64(len(raw)), limits)
			if err != nil {
				t.Fatal(err)
			}
			values, err := r.Values(ctx, 9, 0, 1)
			if err != nil || len(values) != 1 || values[0] != tc.want {
				t.Fatalf("raw values=%v err=%v want=%d", values, err, tc.want)
			}
			v, err := r.Bind(ctx, definition(t, "int ID\nint Value", "$noninline,id$ID\nValue<u32>"), "12.1.0.69875")
			if err != nil {
				t.Fatal(err)
			}
			row, err := v.Row(ctx, 9, 100)
			if err != nil || row["Value"] != tc.want || row["ID"] != uint64(9) {
				t.Fatalf("row=%v err=%v", row, err)
			}
		})
	}
}
