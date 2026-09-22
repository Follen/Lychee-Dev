package relational

import (
	"encoding/json"
	"errors"
	"math"
	"math/big"
	"reflect"
	"strings"
	"testing"
)

func TestScalarExactNumbers(t *testing.T) {
	for _, test := range []struct {
		op         string
		a, b, want any
	}{
		{"-", uint64(math.MaxUint64), uint64(math.MaxUint64 - 1), int64(1)},
		{"+", int64(math.MaxInt64), int64(1), uint64(1) << 63},
		{"%", uint64(math.MaxUint64), int64(2), int64(1)},
		{"%", int64(-7), int64(3), int64(-1)},
		{"/", int64(3), int64(2), 1.5},
		{"/", uint64(math.MaxUint64), uint64(math.MaxUint64), float64(1)},
		{"=", uint64(9007199254740993), float64(9007199254740992), false},
		{"<", uint64(math.MaxUint64), math.Ldexp(1, 64), true},
		{"<", int64(-1), uint64(0), true},
		{"/", int64(1), int64(0), nil},
		{"%", int64(1), int64(0), nil},
		{"=", nil, int64(1), nil},
	} {
		got, err := scalarBinary(test.op, test.a, test.b)
		if err != nil || !reflect.DeepEqual(got, test.want) {
			t.Fatalf("%v %s %v = %#v, %v; want %#v", test.a, test.op, test.b, got, err, test.want)
		}
	}
	for _, test := range []struct {
		op   string
		a, b any
		want error
	}{
		{"+", uint64(math.MaxUint64), 1, ErrNumericRange},
		{"-", int64(math.MinInt64), 1, ErrNumericRange},
		{"*", math.MaxFloat64, 2, ErrNumericRange},
		{"=", "1", 1, ErrType},
		{"+", true, 1, ErrType},
		{"AND", false, 1, ErrType},
		{"OR", true, "true", ErrType},
		{"unknown", nil, nil, ErrType},
		{"||", strings.Repeat("a", 1<<20), "b", ErrBudget},
	} {
		if _, err := scalarBinary(test.op, test.a, test.b); !errors.Is(err, test.want) {
			t.Fatalf("%s: %v, want %v", test.op, err, test.want)
		}
	}
}

func TestScalarTruthTables(t *testing.T) {
	values := []any{nil, false, true}
	wantAnd := [][]any{{nil, false, nil}, {false, false, false}, {nil, false, true}}
	wantOr := [][]any{{nil, nil, true}, {nil, false, true}, {true, true, true}}
	for i, a := range values {
		for j, b := range values {
			for op, table := range map[string][][]any{"AND": wantAnd, "OR": wantOr} {
				got, err := scalarBinary(op, a, b)
				if err != nil || !reflect.DeepEqual(got, table[i][j]) {
					t.Fatalf("%v %s %v = %v, %v", a, op, b, got, err)
				}
			}
		}
	}
	for i, a := range values {
		got, err := scalarUnary("NOT", a)
		if err != nil || !reflect.DeepEqual(got, []any{nil, true, false}[i]) {
			t.Fatal(got, err)
		}
	}
	if got, err := scalarUnary("-", uint64(1)<<63); err != nil || got != int64(math.MinInt64) {
		t.Fatal(got, err)
	}
	if _, err := scalarUnary("-", uint64(math.MaxUint64)); !errors.Is(err, ErrNumericRange) {
		t.Fatal(err)
	}
}

func TestScalarInputBoundary(t *testing.T) {
	for _, value := range []any{math.NaN(), math.Inf(1), math.Inf(-1), json.Number("1e999"), json.Number("18446744073709551616")} {
		if _, err := canonicalScalar(value); !errors.Is(err, ErrNumericRange) {
			t.Fatalf("%v: %v", value, err)
		}
	}
	for _, value := range []any{[]int{1}, map[string]int{"a": 1}, json.Number("1x"), json.Number("--1")} {
		if _, err := canonicalScalar(value); !errors.Is(err, ErrType) {
			t.Fatalf("%v: %v", value, err)
		}
	}
	if got, err := literalNumber("18446744073709551615"); err != nil || got != uint64(math.MaxUint64) {
		t.Fatal(got, err)
	}
	if _, err := literalNumber(strings.Repeat("1", 1025)); !errors.Is(err, ErrNumericRange) {
		t.Fatal(err)
	}
	if _, err := orderScalars(nil, nil); !errors.Is(err, ErrType) {
		t.Fatal(err)
	}
}

func FuzzScalarIntegerArithmetic(f *testing.F) {
	f.Add(uint64(math.MaxUint64), uint64(1))
	f.Add(uint64(9007199254740993), uint64(9007199254740992))
	f.Fuzz(func(t *testing.T, a, b uint64) {
		x, y := new(big.Int).SetUint64(a), new(big.Int).SetUint64(b)
		order, err := orderScalars(a, b)
		if err != nil || order != x.Cmp(y) {
			t.Fatal(order, err)
		}
		for _, op := range []string{"+", "-", "*"} {
			want := new(big.Int)
			switch op {
			case "+":
				want.Add(x, y)
			case "-":
				want.Sub(x, y)
			case "*":
				want.Mul(x, y)
			}
			got, err := scalarBinary(op, a, b)
			if !want.IsInt64() && !(want.Sign() >= 0 && want.IsUint64()) {
				if !errors.Is(err, ErrNumericRange) {
					t.Fatal(op, got, err)
				}
				continue
			}
			if err != nil {
				t.Fatal(err)
			}
			actual, err := numberValue(got)
			if err != nil || actual.exact.Cmp(new(big.Rat).SetInt(want)) != 0 {
				t.Fatal(op, got, want, err)
			}
		}
	})
}
