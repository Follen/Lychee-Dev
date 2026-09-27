package navigatetest

import (
	"context"
	"encoding/binary"
	"github.com/follenfang/lycheedev/internal/records"
	"os"
	"path/filepath"
	"reflect"
	"strconv"
	"strings"
	"testing"
)

func TestExplicitEffectiveSQLKeepsBaseAndProvenance(t *testing.T) {
	spec := fixtureTable{name: "Sample", fileDataID: 201, tableHash: 0xf00d0000, layoutHash: 0xabcd0000, columns: []fixtureColumn{{name: "ID", kind: 'i', bits: 32, identity: true}, {name: "Value", kind: 'i', bits: 32}}, rows: [][]any{{int64(1), int64(42)}, {int64(2), int64(55)}}}
	fx := newFixture(t, []fixtureTable{spec})
	ctx := context.Background()
	makeCapture := func(changes [][4]int64) string {
		raw := make([]byte, 44)
		copy(raw, "XFTH")
		binary.LittleEndian.PutUint32(raw[4:], 9)
		version := strings.Split(fx.pin.Data.FullBuild, ".")
		build, _ := strconv.ParseUint(version[3], 10, 32)
		binary.LittleEndian.PutUint32(raw[8:], uint32(build))
		for _, c := range changes {
			header := make([]byte, 32)
			copy(header, "XFTH")
			binary.LittleEndian.PutUint32(header[8:], uint32(c[0]))
			binary.LittleEndian.PutUint32(header[16:], spec.tableHash)
			binary.LittleEndian.PutUint32(header[20:], uint32(c[1]))
			header[28] = uint8(c[2])
			var payload []byte
			if c[2] == 1 {
				payload = binary.LittleEndian.AppendUint32(payload, uint32(c[1]))
				payload = binary.LittleEndian.AppendUint32(payload, uint32(c[3]))
			}
			binary.LittleEndian.PutUint32(header[24:], uint32(len(payload)))
			raw = append(raw, header...)
			raw = append(raw, payload...)
		}
		path := filepath.Join(t.TempDir(), "cache.bin")
		if err := os.WriteFile(path, raw, 0600); err != nil {
			t.Fatal(err)
		}
		reading, err := records.InspectHotfix(ctx, fx.workspace, fx.pin.ID, records.HotfixRequest{File: path, Table: "Sample", Offline: true, Filter: records.CacheFilter{Limit: 200}, MaxBytes: 1 << 20})
		if err != nil {
			t.Fatal(err)
		}
		return reading.Result.Source.ID
	}
	first := makeCapture([][4]int64{{1, 1, 1, 99}, {2, 2, 2, 0}, {3, 3, 1, 77}, {-1, 1, 1, 10}})
	second := makeCapture([][4]int64{{1, 1, 1, 100}})
	for _, tc := range []struct {
		sql    string
		hotfix []string
		want   [][]any
	}{
		{"SELECT ID,Value FROM Sample ORDER BY ID", []string{first}, [][]any{{uint64(1), uint64(42)}, {uint64(2), uint64(55)}}},
		{"SELECT ID,Value FROM effective.Sample ORDER BY ID", []string{first}, [][]any{{uint64(1), uint64(99)}, {uint64(3), uint64(77)}}},
		{"SELECT ID,Value FROM effective.Sample WHERE ID=1.0", []string{first, second}, [][]any{{uint64(1), uint64(100)}}},
		{"SELECT s.ID,s.Value,e.Value FROM Sample s LEFT JOIN effective.Sample e ON s.ID=e.ID ORDER BY s.ID", []string{first}, [][]any{{uint64(1), uint64(42), uint64(99)}, {uint64(2), uint64(55), nil}}},
	} {
		got, err := records.QueryData(ctx, fx.workspace, fx.pin.ID, fixtureQuery(), records.DataQuery{SQL: tc.sql, Hotfix: tc.hotfix})
		if err != nil || !reflect.DeepEqual(got.Result.Result.Rows, tc.want) {
			t.Fatalf("%s: %#v %v", tc.sql, got.Result.Result.Rows, err)
		}
		if strings.Contains(tc.sql, "effective.") && (len(got.Result.EffectiveSources) != 1 || got.Result.EffectiveSources[0].Deleted != 1) {
			t.Fatal("missing overlay provenance")
		}
	}
	got, err := records.QueryData(ctx, fx.workspace, fx.pin.ID, fixtureQuery(), records.DataQuery{SQL: "SELECT __hotfix_source,__hotfix_push FROM effective.Sample WHERE ID=1", Hotfix: []string{first}})
	if err != nil || !reflect.DeepEqual(got.Result.Result.Rows, [][]any{{first, int64(1)}}) {
		t.Fatalf("%#v %v", got.Result.Result.Rows, err)
	}
}
