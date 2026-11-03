package sqlengine

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"math"
	"reflect"
	"strings"
	"sync/atomic"
	"testing"
	"time"
)

var testLimits = Limits{MemoryBytes: 64 << 20, ResultBytes: 16 << 20, SourceRows: 10000000}

func executeTest(t *testing.T, query string, params map[string]any, resolve Resolver) (Result, error) {
	t.Helper()
	q, err := Open(context.Background(), query, params, testLimits)
	if err != nil {
		return Result{}, err
	}
	defer q.Close()
	return q.Execute(context.Background(), resolve)
}

func fixedSource(rows [][]any) Source {
	return Source{Columns: []string{"ID", "Value", "Name"}, Types: []Kind{Unsigned, Signed, Text}, Identity: "ID",
		Scan: func(ctx context.Context, yield func([]any) error) error {
			for _, row := range rows {
				if err := ctx.Err(); err != nil {
					return err
				}
				if err := yield(row); err != nil {
					return err
				}
			}
			return nil
		},
	}
}

func TestDuckDBIndexedPointAndSetQueries(t *testing.T) {
	for _, query := range []string{
		"SELECT Name FROM SpellName WHERE ID=35",
		"SELECT Name FROM SpellName WHERE Value=70 AND ID=35",
		"SELECT Name FROM static.SpellName WHERE ID=$id",
		"SELECT Name FROM SpellName WHERE ID IN (35,35,NULL) AND Value=70",
		"SELECT Name FROM SpellName WHERE ID IN (1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31,32,33,34,35) ORDER BY ID",
	} {
		t.Run(query, func(t *testing.T) {
			var scans, lookups int
			source := fixedSource(nil)
			source.Scan = func(context.Context, func([]any) error) error {
				scans++
				return errors.New("point query scanned the full source")
			}
			source.Equal = func(_ context.Context, column string, value any, yield func([]any) error) (bool, error) {
				if column != "ID" {
					return false, nil
				}
				lookups++
				id := value.(uint64)
				return true, yield([]any{id, int64(id * 2), fmt.Sprintf("spell-%d", id)})
			}
			var params map[string]any
			if strings.Contains(query, "$id") {
				params = map[string]any{"id": json.Number("35")}
			}
			result, err := executeTest(t, query, params, func(_ context.Context, use TableUse) (Source, error) { return source, nil })
			want := 1
			if strings.Contains(query, "1,2,3,4") {
				want = 35
			}
			if err != nil || scans != 0 || lookups != want || len(result.Rows) != want || result.Rows[want-1][0] != "spell-35" {
				t.Fatalf("scans=%d lookups=%d result=%+v err=%v", scans, lookups, result, err)
			}
		})
	}
}

func TestDuckDBTypesRemainExact(t *testing.T) {
	result, err := executeTest(t, `SELECT $unsigned::UBIGINT AS u, $signed::BIGINT AS s, 1.25::DECIMAL(10,2) AS d, [1,NULL,3] AS a`, map[string]any{"unsigned": json.Number("18446744073709551615"), "signed": json.Number("-9223372036854775808")}, nil)
	if err != nil {
		t.Fatal(err)
	}
	want := [][]any{{uint64(math.MaxUint64), int64(math.MinInt64), json.Number("1.25"), []any{int64(1), nil, int64(3)}}}
	if !reflect.DeepEqual(result.Rows, want) {
		t.Fatalf("%#v", result.Rows)
	}
	raw, err := json.Marshal(result.Rows)
	if err != nil || !strings.Contains(string(raw), "18446744073709551615") || !strings.Contains(string(raw), "-9223372036854775808") || !strings.Contains(string(raw), "1.25") {
		t.Fatalf("%s %v", raw, err)
	}
	if !reflect.DeepEqual(result.Types, []string{"UBIGINT", "BIGINT", "DECIMAL(10,2)", "INTEGER[]"}) {
		t.Fatal(result.Types)
	}
}

func TestDuckDBJoinsGroupingCTEsAndSubqueries(t *testing.T) {
	resolve := func(_ context.Context, use TableUse) (Source, error) {
		switch strings.ToLower(use.Name) {
		case "sample":
			return fixedSource([][]any{{uint64(1), int64(10), "a"}, {uint64(2), int64(20), "b"}, {uint64(3), int64(20), "c"}}), nil
		case "related":
			return fixedSource([][]any{{uint64(1), int64(100), "x"}, {uint64(1), int64(200), "y"}}), nil
		}
		return Source{}, fmt.Errorf("unexpected source %v", use)
	}
	for _, tc := range []struct {
		sql  string
		rows [][]any
	}{
		{`SELECT s.ID,r.Value FROM Sample s LEFT JOIN Related r ON s.ID=r.ID ORDER BY s.ID,r.Value`, [][]any{{uint64(1), int64(100)}, {uint64(1), int64(200)}, {uint64(2), nil}, {uint64(3), nil}}},
		{`WITH t AS (SELECT Value,COUNT(*) n FROM Sample GROUP BY Value) SELECT * FROM t ORDER BY Value`, [][]any{{int64(10), int64(1)}, {int64(20), int64(2)}}},
		{`SELECT ID,(SELECT COUNT(*) FROM Related) AS n FROM Sample ORDER BY ID`, [][]any{{uint64(1), int64(2)}, {uint64(2), int64(2)}, {uint64(3), int64(2)}}},
		{`SELECT s.ID,(SELECT COUNT(*) FROM Related r WHERE r.ID=s.ID) AS n FROM Sample s ORDER BY ID`, [][]any{{uint64(1), int64(2)}, {uint64(2), int64(0)}, {uint64(3), int64(0)}}},
		{`SELECT DISTINCT Value FROM Sample ORDER BY Value LIMIT 1 OFFSET 1`, [][]any{{int64(20)}}},
		{`WITH Sample AS (SELECT ID FROM Sample WHERE ID=2) SELECT ID FROM Sample`, [][]any{{uint64(2)}}},
		{`WITH RECURSIVE t(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM t WHERE x<3) SELECT x FROM t ORDER BY x`, [][]any{{int64(1)}, {int64(2)}, {int64(3)}}},
	} {
		t.Run(tc.sql, func(t *testing.T) {
			result, err := executeTest(t, tc.sql, nil, resolve)
			if err != nil || !reflect.DeepEqual(result.Rows, tc.rows) {
				t.Fatalf("got=%#v want=%#v err=%v", result.Rows, tc.rows, err)
			}
		})
	}
}

func TestDuckDBReadOnlyAdmissionAndNoLegacyDialect(t *testing.T) {
	for _, query := range []string{"DELETE FROM x", "SELECT 1; SELECT 2", "CREATE TABLE x AS SELECT 1", "EXPLAIN ANALYZE DELETE FROM x", "SELECT :old", "SELECT * FROM other.x"} {
		q, err := Open(context.Background(), query, nil, testLimits)
		if err == nil {
			q.Close()
			t.Fatalf("admitted %s", query)
		}
	}
	for _, query := range []string{"SELECT * FROM read_csv_auto('should-not-open.csv')", "SELECT * FROM read_csv_auto('https://example.invalid/data.csv')"} {
		_, err := executeTest(t, query, nil, nil)
		if err == nil || !strings.Contains(err.Error(), "disabled") {
			t.Fatalf("external read: %s %v", query, err)
		}
	}
}

func TestDuckDBNativeCancellationAndEarlyScanCleanup(t *testing.T) {
	q, err := Open(context.Background(), "SELECT sum(i) FROM range(1000000000000) t(i)", nil, testLimits)
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Millisecond)
	_, err = q.Execute(ctx, nil)
	cancel()
	q.Close()
	if !errors.Is(err, context.DeadlineExceeded) {
		t.Fatal(err)
	}
	var active atomic.Int64
	source := fixedSource(nil)
	source.Scan = func(ctx context.Context, yield func([]any) error) error {
		active.Add(1)
		defer active.Add(-1)
		for i := uint64(0); ; i++ {
			if err := yield([]any{i, int64(i), "x"}); err != nil {
				return err
			}
		}
	}
	result, err := executeTest(t, "SELECT ID FROM Sample LIMIT 1", nil, func(context.Context, TableUse) (Source, error) { return source, nil })
	if err != nil || len(result.Rows) != 1 || active.Load() != 0 {
		t.Fatalf("active=%d result=%+v err=%v", active.Load(), result, err)
	}
}

func TestDuckDBProjectionAndExplain(t *testing.T) {
	var projected []int
	source := fixedSource(nil)
	source.Scan = func(context.Context, func([]any) error) error { t.Error("unused full scan"); return nil }
	source.ScanSelected = func(_ context.Context, indices []int, yield func([]any) error) error {
		projected = append([]int{}, indices...)
		return yield([]any{nil, nil, "a"})
	}
	resolve := func(context.Context, TableUse) (Source, error) { return source, nil }
	result, err := executeTest(t, "EXPLAIN SELECT Name FROM Sample", nil, resolve)
	if err != nil || projected != nil || result.Plan == nil || result.Plan.Engine != "duckdb" || len(result.Plan.Entries) == 0 {
		t.Fatalf("plan=%+v projected=%v err=%v", result.Plan, projected, err)
	}
	result, err = executeTest(t, "/*lead*/ EXPLAIN /*inner*/ ANALYZE SELECT Name FROM Sample", nil, resolve)
	if err != nil || !reflect.DeepEqual(projected, []int{2}) || result.Plan == nil || !result.Plan.Analyze || len(result.Scans) != 1 || result.Scans[0].Rows != 1 {
		t.Fatalf("result=%+v projected=%v err=%v", result, projected, err)
	}
}

func TestDuckDBIndexHintsPreserveOtherScopes(t *testing.T) {
	source := fixedSource([][]any{{uint64(1), int64(10), "a"}, {uint64(2), int64(20), "b"}, {uint64(3), int64(30), "c"}})
	source.Equal = func(context.Context, string, any, func([]any) error) (bool, error) {
		return false, errors.New("unsafe index restriction")
	}
	for _, tc := range []struct {
		sql  string
		rows [][]any
	}{
		{"SELECT ID FROM Sample WHERE ID=1 OR Value=20 ORDER BY ID", [][]any{{uint64(1)}, {uint64(2)}}},
		{"SELECT (SELECT max(ID) FROM Sample) FROM Sample WHERE ID=1", [][]any{{uint64(3)}}},
		{"WITH chosen AS (SELECT ID FROM Sample WHERE ID=1) SELECT max(ID) FROM Sample,chosen", nil},
		{"SELECT a.ID,b.ID FROM Sample a JOIN Sample b ON a.ID=1 AND b.ID=3", [][]any{{uint64(1), uint64(3)}}},
	} {
		t.Run(tc.sql, func(t *testing.T) {
			result, err := executeTest(t, tc.sql, nil, func(context.Context, TableUse) (Source, error) { return source, nil })
			if tc.rows == nil {
				if !errors.Is(err, ErrBinding) {
					t.Fatalf("ambiguous ID accepted: %v", err)
				}
			} else if err != nil || !reflect.DeepEqual(result.Rows, tc.rows) {
				t.Fatalf("rows=%#v want=%#v err=%v", result.Rows, tc.rows, err)
			}
		})
	}
}

func TestDuckDBColumnAliasesPreserveIdentityMeaning(t *testing.T) {
	rows := [][]any{{uint64(1), int64(10), "a"}, {uint64(2), int64(20), "b"}}
	source := fixedSource(rows)
	source.Equal = func(_ context.Context, _ string, value any, yield func([]any) error) (bool, error) {
		for _, row := range rows {
			if row[0] == value {
				return true, yield(row)
			}
		}
		return true, nil
	}
	for _, query := range []string{
		"SELECT original_id,ID FROM Sample AS s(original_id,ID,Name) WHERE ID=20",
		"SELECT original_id,ID FROM Sample AS s(original_id,ID,Name) WHERE s.ID IN (20)",
	} {
		t.Run(query, func(t *testing.T) {
			result, err := executeTest(t, query, nil, func(context.Context, TableUse) (Source, error) { return source, nil })
			want := [][]any{{uint64(2), int64(20)}}
			if err != nil || !reflect.DeepEqual(result.Rows, want) {
				t.Fatalf("rows=%#v want=%#v err=%v", result.Rows, want, err)
			}
		})
	}
}

func TestDuckDBErrorAndBudgetBoundaries(t *testing.T) {
	for _, tc := range []struct {
		sql  string
		want error
	}{
		{"SELECT (SELECT * FROM range(2))", ErrCardinality},
		{"SELECT regexp_matches('x','[')", ErrType},
		{"SELECT 'text'::BIGINT", ErrType},
		{"SELECT 9223372036854775807::BIGINT + 1", ErrNumericRange},
	} {
		_, err := executeTest(t, tc.sql, nil, nil)
		if !errors.Is(err, tc.want) {
			t.Fatalf("%s: %v; want %v", tc.sql, err, tc.want)
		}
	}
	for _, limit := range []Limits{
		{MemoryBytes: 64 << 20, ResultBytes: 1, SourceRows: 10},
		{MemoryBytes: 64 << 20, ResultBytes: 1024, SourceRows: 1},
	} {
		q, err := Open(context.Background(), "SELECT * FROM Sample", nil, limit)
		if err != nil {
			t.Fatal(err)
		}
		_, err = q.Execute(context.Background(), func(context.Context, TableUse) (Source, error) {
			return fixedSource([][]any{{uint64(1), int64(10), "a"}, {uint64(2), int64(20), "b"}}), nil
		})
		q.Close()
		if !errors.Is(err, ErrBudget) {
			t.Fatalf("%+v: %v", limit, err)
		}
	}
	want := errors.New("reader source changed")
	source := fixedSource(nil)
	source.Scan = func(context.Context, func([]any) error) error { return want }
	_, err := executeTest(t, "SELECT * FROM Sample", nil, func(context.Context, TableUse) (Source, error) { return source, nil })
	if !errors.Is(err, want) {
		t.Fatalf("reader error lost: %v", err)
	}
}
