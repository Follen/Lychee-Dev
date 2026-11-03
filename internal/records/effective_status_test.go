package records

import (
	"context"
	"encoding/binary"
	"fmt"
	"reflect"
	"testing"

	"github.com/follenfang/lycheedev/internal/records/schema"
	"github.com/follenfang/lycheedev/internal/records/sqlengine"
	"github.com/follenfang/lycheedev/internal/selection"
)

func TestEffectiveNotPublicPreservesPriorState(t *testing.T) {
	for _, layout := range []uint32{7, 8, 9} {
		for _, tc := range []struct {
			name              string
			states            []uint8
			want              [][]any
			deleted, replaced int
		}{
			{"base", []uint8{4}, [][]any{{uint64(1), uint64(42)}}, 0, 0},
			{"valid", []uint8{1, 4}, [][]any{{uint64(1), uint64(99)}}, 0, 1},
			{"deleted", []uint8{2, 4}, nil, 1, 0},
			{"then-valid", []uint8{4, 1}, [][]any{{uint64(1), uint64(99)}}, 0, 1},
		} {
			t.Run(fmt.Sprintf("v%d/%s", layout, tc.name), func(t *testing.T) {
				ctx := context.Background()
				snapshots := newHotfixSnapshots(nil, nil, selection.DataPin{}, nil)
				var captures []string
				for i, state := range tc.states {
					id := fmt.Sprintf("capture-%d", i)
					captures = append(captures, id)
					payload := binary.LittleEndian.AppendUint32(nil, 99)
					snapshots.captures[id] = &hotfixSnapshot{layout: layout, raw: payload, tables: map[uint32][]CacheEntry{7: {{RecordID: 1, Status: state, Push: int32(i), PayloadBytes: len(payload)}}}}
				}
				base := sqlengine.Source{Columns: []string{"ID", "Value"}, Scan: func(_ context.Context, yield func([]any) error) error { return yield([]any{uint64(1), uint64(42)}) }}
				definition := schema.Definition{Fields: []schema.Field{{Name: "ID", Kind: "int", Identity: true, Elements: 1, Bits: 32}, {Name: "Value", Kind: "int", Inline: true, Elements: 1, Bits: 32}}}
				bundle := DefinitionBundle{}
				bundle.Identity.Name, bundle.Identity.Hash = "Sample", 7
				source, report, err := effectiveSourceWithSnapshots(ctx, snapshots, base, definition, bundle, captures)
				if err != nil {
					t.Fatal(err)
				}
				var rows [][]any
				if err := source.Scan(ctx, func(row []any) error { rows = append(rows, row[:2]); return nil }); err != nil {
					t.Fatal(err)
				}
				if !reflect.DeepEqual(rows, tc.want) || report.Deleted != tc.deleted || report.ReplacedOrAdded != tc.replaced {
					t.Fatalf("rows=%#v deleted=%d replaced=%d", rows, report.Deleted, report.ReplacedOrAdded)
				}
			})
		}
	}
}
