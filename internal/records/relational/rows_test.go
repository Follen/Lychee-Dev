package relational

import (
	"context"
	"errors"
	"reflect"
	"testing"
)

func TestRowExecutionPipeline(t *testing.T) {
	fields := []field{{"t", "Kind"}, {"t", "Amount"}}
	input := [][]any{{"a", 2}, {"b", 4}, {"a", 5}, {"c", nil}}
	for _, test := range []struct {
		sql   string
		names []string
		rows  [][]any
	}{
		{"SELECT kind, SUM(amount) AS total FROM t WHERE amount IS NOT NULL GROUP BY t.KIND HAVING SUM(amount)>3 ORDER BY SUM(amount) DESC LIMIT 1", []string{"Kind", "total"}, [][]any{{"a", int64(7)}}},
		{"SELECT * FROM t WHERE Amount > 2 ORDER BY Amount DESC", []string{"Kind", "Amount"}, [][]any{{"a", int64(5)}, {"b", int64(4)}}},
		{"SELECT DISTINCT Kind FROM t ORDER BY Kind LIMIT 2", []string{"Kind"}, [][]any{{"a"}, {"b"}}},
		{"SELECT Kind, SUM(Amount) AS total FROM t GROUP BY Kind ORDER BY total DESC LIMIT 1", []string{"Kind", "total"}, [][]any{{"a", int64(7)}}},
	} {
		p, err := Compile(context.Background(), test.sql)
		if err != nil {
			t.Fatal(err)
		}
		e := evaluation{ctx: context.Background(), remaining: 100000, retainedBytes: 1 << 20}
		got, err := executeRows(&e, p.root, fields, func(yield func([]any) error) error {
			for _, row := range input {
				if err := yield(row); err != nil {
					return err
				}
			}
			return nil
		})
		if err != nil || !reflect.DeepEqual(got.names, test.names) || !reflect.DeepEqual(got.rows, test.rows) {
			t.Fatalf("%s: %#v %v", test.sql, got, err)
		}
		if e.column != nil || e.resolved != nil {
			t.Fatal("scope leaked")
		}
	}
}

func TestBindBeforeEmptyScan(t *testing.T) {
	for _, sql := range []string{"SELECT missing FROM t", "SELECT CASE WHEN FALSE THEN BAD() ELSE 1 END FROM t", "SELECT SUM(COUNT(*)) FROM t", "SELECT x FROM t WHERE SUM(x)>0", "SELECT CAST(NULL AS BAD) FROM t", "SELECT :missing FROM t"} {
		p, err := Compile(context.Background(), sql)
		if err != nil {
			t.Fatal(err)
		}
		e := evaluation{ctx: context.Background(), remaining: 10000, retainedBytes: 100000}
		scanned := false
		_, err = executeRows(&e, p.root, []field{{"t", "x"}}, func(func([]any) error) error { scanned = true; return nil })
		if err == nil || scanned {
			t.Fatal(sql, err, scanned)
		}
	}
}

func TestRowStreamErrorAtomic(t *testing.T) {
	p, err := Compile(context.Background(), "SELECT x FROM t")
	if err != nil {
		t.Fatal(err)
	}
	e := evaluation{ctx: context.Background(), remaining: 10000, retainedBytes: 100000}
	failure := errors.New("read failed")
	result, err := executeRows(&e, p.root, []field{{"t", "x"}}, func(yield func([]any) error) error {
		if err := yield([]any{1}); err != nil {
			return err
		}
		return failure
	})
	if !errors.Is(err, failure) || result.rows != nil || result.names != nil {
		t.Fatal(result, err)
	}
}
