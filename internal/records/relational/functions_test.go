package relational

import (
	"context"
	"errors"
	"testing"
)

func TestFunctionAndCastFailures(t *testing.T) {
	for _, test := range []struct {
		source string
		want   error
	}{
		{"LOWER(1)", ErrType},
		{"UPPER('a', 'b')", ErrType},
		{"LENGTH()", ErrType},
		{"COALESCE()", ErrType},
		{"NULLIF(1)", ErrType},
		{"MISSING(NULL)", ErrUnsupported},
		{"CAST(NULL AS MISSING)", ErrUnsupported},
		{"CAST('abc' AS REAL)", ErrType},
		{"CAST(18446744073709551615 AS BIGINT)", ErrNumericRange},
		{"CAST(-9223372036854775809 AS BIGINT)", ErrNumericRange},
		{"CAST('1e999' AS REAL)", ErrNumericRange},
		{"CAST(2 AS BOOL)", ErrType},
		{"LOWER(DISTINCT 'x')", ErrUnsupported},
	} {
		program, err := Compile(context.Background(), "SELECT "+test.source+" FROM t")
		if err != nil {
			t.Fatal(test.source, err)
		}
		e := evaluation{ctx: context.Background(), remaining: 1000}
		if _, err := e.value(program.root.outputs[0].value); !errors.Is(err, test.want) {
			t.Fatalf("%s: %v, want %v", test.source, err, test.want)
		}
	}
}
