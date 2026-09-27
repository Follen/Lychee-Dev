package relational

import (
	"context"
	"reflect"
	"testing"
)

func TestFlagsPreserveExactBitsAndUnknown(t *testing.T) {
	p, err := Compile(context.Background(), "SELECT HAS_FLAG(9223372036854775808,9223372036854775808),BIT_AND(18446744073709551615,9223372036854775808),HAS_FLAG(NULL,1),HAS_FLAG(4,2) FROM a LIMIT 1")
	if err != nil {
		t.Fatal(err)
	}
	got, err := p.Execute(context.Background(), fixtureResolver, nil, Limits{Work: 10000, MemoryBytes: 1 << 20})
	if err != nil || !reflect.DeepEqual(got.Rows, [][]any{{true, uint64(1) << 63, nil, false}}) {
		t.Fatalf("%#v %v", got, err)
	}
}
