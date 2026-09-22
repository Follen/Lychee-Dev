package relational

import (
	"context"
	"errors"
	"reflect"
	"testing"
)

func TestCompiledExpressions(t *testing.T) {
	for _, test := range []struct {
		source string
		want   any
	}{
		{"18446744073709551615 - 18446744073709551614", int64(1)},
		{"2 IN (1, NULL, 2)", true},
		{"2 NOT IN (1, NULL)", nil},
		{"NULL IN (1, 2)", nil},
		{"2 BETWEEN 1 AND 3", true},
		{"2 NOT BETWEEN 1 AND 3", false},
		{"2 BETWEEN 3 AND NULL", false},
		{"NULL IS NULL", true},
		{"0 IS NOT NULL", true},
		{"CASE WHEN NULL THEN 1 WHEN TRUE THEN 2 ELSE 3 END", int64(2)},
		{"CASE NULL WHEN NULL THEN 1 ELSE 2 END", int64(2)},
		{"CASE 2 WHEN 1 THEN 3 WHEN 2 THEN 4 END", int64(4)},
		{"CASE WHEN TRUE THEN 1 ELSE 18446744073709551615 + 1 END", int64(1)},
		{"CASE WHEN FALSE THEN 1 END", nil},
		{":number + t.ID", int64(5)},
		{"LOWER('WARRIOR')", "warrior"},
		{"UPPER('mage')", "MAGE"},
		{"LENGTH('猫x')", int64(2)},
		{"LOWER(NULL)", nil},
		{"COALESCE(NULL, 2, 18446744073709551615 + 1)", int64(2)},
		{"NULLIF(2, NULL)", int64(2)},
		{"NULLIF(2, 2)", nil},
		{"CAST(18446744073709551615 AS TEXT)", "18446744073709551615"},
		{"CAST('-2.75' AS INTEGER)", int64(-2)},
		{"CAST(3 AS REAL)", float64(3)},
		{"CAST('true' AS BOOLEAN)", true},
		{"CAST(NULL AS INTEGER)", nil},
		{"'Warrior' LIKE 'W%r'", true},
		{"'Warrior' NOT LIKE 'w%'", true},
		{"'猫' LIKE '_'", true},
		{"'' LIKE '%%'", true},
		{"'' LIKE '_'", false},
		{"'aab' LIKE '%a_b'", true},
		{"NULL NOT LIKE '%'", nil},
	} {
		program, err := Compile(context.Background(), "SELECT "+test.source+" FROM t")
		if err != nil {
			t.Fatal(test.source, err)
		}
		e := evaluation{ctx: context.Background(), remaining: 1000, parameters: map[string]any{"number": 2}, column: func(q, n string) (any, error) {
			if q != "t" || n != "ID" {
				return nil, ErrBinding
			}
			return 3, nil
		}}
		got, err := e.value(program.root.outputs[0].value)
		if err != nil || !reflect.DeepEqual(got, test.want) {
			t.Fatalf("%s: %#v, %v; want %#v", test.source, got, err, test.want)
		}
	}
}

func TestExpressionExecutionLimits(t *testing.T) {
	matcher := evaluation{ctx: context.Background(), remaining: 4}
	if _, err := matcher.like("aaaaaaaa", "%a%a%a"); !errors.Is(err, ErrBudget) {
		t.Fatal(err)
	}
	program, err := Compile(context.Background(), "SELECT 1 + 2 FROM t")
	if err != nil {
		t.Fatal(err)
	}
	node := program.root.outputs[0].value
	e := evaluation{ctx: context.Background(), remaining: 2}
	if _, err := e.value(node); !errors.Is(err, ErrBudget) {
		t.Fatal(err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	e = evaluation{ctx: ctx, remaining: 100}
	if _, err := e.value(node); !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
	for _, source := range []string{":missing", "t.ID"} {
		program, err := Compile(context.Background(), "SELECT "+source+" FROM t")
		if err != nil {
			t.Fatal(err)
		}
		e = evaluation{ctx: context.Background(), remaining: 100}
		if _, err := e.value(program.root.outputs[0].value); !errors.Is(err, ErrBinding) {
			t.Fatal(source, err)
		}
	}
}
