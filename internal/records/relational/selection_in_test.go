package relational

import (
	"context"
	"errors"
	"fmt"
	"math"
	"reflect"
	"strings"
	"testing"
)

func TestIDInUsesEqualityCandidatesWithinWorkBudget(t *testing.T) {
	var ids []string
	var want [][]any
	for id := 1; id <= 35; id++ {
		ids = append(ids, fmt.Sprint(id))
		want = append(want, []any{int64(id), fmt.Sprintf("spell-%d", id)})
	}
	p, err := Compile(context.Background(), "SELECT ID, Name_lang FROM SpellName WHERE ID IN ("+strings.Join(ids, ",")+") ORDER BY ID")
	if err != nil {
		t.Fatal(err)
	}
	scans, lookups := 0, 0
	resolve := func(context.Context, TableUse) (Source, error) {
		return Source{Columns: []string{"ID", "Name_lang"}, Scan: func(_ context.Context, yield func([]any) error) error {
			scans++
			for id := 1; id <= 1000; id++ {
				if err := yield([]any{int64(id), fmt.Sprintf("spell-%d", id)}); err != nil {
					return err
				}
			}
			return nil
		}, Equal: func(_ context.Context, column string, value any, yield func([]any) error) (bool, error) {
			if column != "ID" {
				return false, nil
			}
			lookups++
			id := value.(int64)
			return true, yield([]any{id, fmt.Sprintf("spell-%d", id)})
		}}, nil
	}
	got, err := p.Execute(context.Background(), resolve, nil, Limits{Work: 5000, MemoryBytes: 1 << 20})
	if err != nil || !reflect.DeepEqual(got.Rows, want) || scans != 0 || lookups != 35 {
		t.Fatalf("rows=%v err=%v scans=%d lookups=%d", got.Rows, err, scans, lookups)
	}
}

// These providers expose the same typed equality and scan over immutable rows.
// The repeated ID rows are distinct source rows, not duplicate IN candidates.
func inTestSource(rows [][]any, equal bool, scans, lookups *int, decline func(string, any) bool) Source {
	s := Source{Columns: []string{"ID", "Name"}, Scan: func(_ context.Context, yield func([]any) error) error {
		*scans++
		for _, row := range rows {
			if err := yield(row); err != nil {
				return err
			}
		}
		return nil
	}}
	if equal {
		s.Equal = func(_ context.Context, column string, value any, yield func([]any) error) (bool, error) {
			*lookups++
			if decline != nil && decline(column, value) {
				return false, nil
			}
			for _, row := range rows {
				match, err := scalarBinary("=", row[0], value)
				if err != nil {
					return true, err
				}
				if match == true {
					if err := yield(row); err != nil {
						return true, err
					}
				}
			}
			return true, nil
		}
	}
	return s
}

func TestInEqualitySelectionMatchesScanSemantics(t *testing.T) {
	rows := [][]any{{int64(1), "a"}, {uint64(1), "b"}, {float64(2), "c"}, {int64(3), "d"}, {nil, "null"}}
	for _, test := range []struct {
		predicate string
		params    map[string]any
	}{
		{"ID IN (2, 1, 1.0, :one, NULL, 99)", map[string]any{"one": uint64(1)}},
		{"ID IN (:two, :none, :negative)", map[string]any{"two": 2.0, "none": nil, "negative": int64(-1)}},
		{"ID IN (-1, +1, 2.0, 2.5)", nil},
		{"ID IN (:bad)", map[string]any{"bad": true}},
		{"ID IN (NULL, NULL)", nil},
		{"ID IN (1, 'bad')", nil},
		{"ID IN (1, 18446744073709551616)", nil},
		{"ID IN (1, 1 / 0)", nil},
		{"ID NOT IN (1, 2)", nil},
		{"ID IN (SELECT ID FROM t WHERE ID=2)", nil},
	} {
		t.Run(test.predicate, func(t *testing.T) {
			p, err := Compile(context.Background(), "SELECT ID,Name FROM t WHERE "+test.predicate+" ORDER BY ID,Name")
			if err != nil {
				t.Fatal(err)
			}
			var results [2]Result
			var failures [2]error
			for i, equal := range []bool{false, true} {
				scans, lookups := 0, 0
				resolve := func(context.Context, TableUse) (Source, error) {
					return inTestSource(rows, equal, &scans, &lookups, nil), nil
				}
				results[i], failures[i] = p.Execute(context.Background(), resolve, test.params, Limits{Work: 100000, MemoryBytes: 1 << 20})
			}
			if !reflect.DeepEqual(results[0], results[1]) || (failures[0] == nil) != (failures[1] == nil) ||
				failures[0] != nil && !errors.Is(failures[1], failures[0]) {
				t.Fatalf("scan=%+v %v; indexed=%+v %v", results[0], failures[0], results[1], failures[1])
			}
		})
	}
}

func TestInEqualitySelectionCopiesRowsAndDeduplicatesExactNumbers(t *testing.T) {
	p, err := Compile(context.Background(), "SELECT ID,Name FROM t WHERE ID IN (:high, :round, :duplicate, 1) ORDER BY ID")
	if err != nil {
		t.Fatal(err)
	}
	lookups := 0
	shared := make([]any, 2)
	resolve := func(context.Context, TableUse) (Source, error) {
		return Source{Columns: []string{"ID", "Name"}, Scan: func(context.Context, func([]any) error) error {
			t.Fatal("indexed numeric list fell back to scan")
			return nil
		}, Equal: func(_ context.Context, _ string, value any, yield func([]any) error) (bool, error) {
			lookups++
			// The rounded float is different from MaxUint64 and has no row.
			if value == float64(math.MaxUint64) {
				return true, nil
			}
			shared[0], shared[1] = value, fmt.Sprint(value)
			return true, yield(shared)
		}}, nil
	}
	got, err := p.Execute(context.Background(), resolve,
		map[string]any{"high": uint64(math.MaxUint64), "round": float64(math.MaxUint64), "duplicate": float64(1)},
		Limits{Work: 10000, MemoryBytes: 1 << 20})
	want := [][]any{{float64(1), "1"}, {uint64(math.MaxUint64), "18446744073709551615"}}
	if err != nil || !reflect.DeepEqual(got.Rows, want) || lookups != 3 {
		t.Fatalf("rows=%v err=%v lookups=%d", got.Rows, err, lookups)
	}
}

func TestInEqualitySelectionBufferLimitFallsBackToStreaming(t *testing.T) {
	p, err := Compile(context.Background(), "SELECT COUNT(*) FROM t WHERE ID IN (1,2)")
	if err != nil {
		t.Fatal(err)
	}
	scans, lookups := 0, 0
	rows := [][]any{{int64(1), strings.Repeat("x", 1800)}, {int64(2), strings.Repeat("y", 1800)}}
	resolve := func(context.Context, TableUse) (Source, error) {
		return inTestSource(rows, true, &scans, &lookups, nil), nil
	}
	got, err := p.Execute(context.Background(), resolve, nil, Limits{Work: 10000, MemoryBytes: 2048})
	if err != nil || !reflect.DeepEqual(got.Rows, [][]any{{int64(2)}}) || scans != 1 || lookups != 1 {
		t.Fatalf("rows=%v err=%v scans=%d lookups=%d", got.Rows, err, scans, lookups)
	}
}

func TestInEqualitySelectionDeclineDiscardsEarlierCandidates(t *testing.T) {
	for _, predicate := range []string{"ID IN (1, 2)", "Name IN (1, 2)"} {
		t.Run(predicate, func(t *testing.T) {
			p, err := Compile(context.Background(), "SELECT ID,Name FROM t WHERE "+predicate+" ORDER BY ID")
			if err != nil {
				t.Fatal(err)
			}
			scans, lookups := 0, 0
			rows := [][]any{{int64(1), int64(1)}, {int64(2), int64(2)}}
			resolve := func(context.Context, TableUse) (Source, error) {
				return inTestSource(rows, true, &scans, &lookups, func(column string, value any) bool {
					return column != "ID" || value == int64(2)
				}), nil
			}
			got, err := p.Execute(context.Background(), resolve, nil, Limits{Work: 10000, MemoryBytes: 1 << 20})
			if err != nil || !reflect.DeepEqual(got.Rows, rows) || scans != 1 {
				t.Fatalf("rows=%v err=%v scans=%d lookups=%d", got.Rows, err, scans, lookups)
			}
		})
	}
}
