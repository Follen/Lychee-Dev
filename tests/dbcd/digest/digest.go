// Package digest implements the versioned, language-independent field stream
// used by the test-only DBCD adapter. It never converts integers to float64.
package digest

import (
	"fmt"
	"io"
	"math"
	"reflect"
	"sort"
)

func Row(w io.Writer, id uint32, row map[string]any) {
	value(w, uint64(id))
	names := make([]string, 0, len(row))
	for name := range row {
		names = append(names, name)
	}
	sort.Strings(names)
	for _, name := range names {
		value(w, name)
		value(w, row[name])
	}
}
func value(w io.Writer, v any) {
	switch x := v.(type) {
	case nil:
		fmt.Fprint(w, "z;")
	case string:
		fmt.Fprintf(w, "s%d:%s", len(x), x)
	case float64:
		fmt.Fprintf(w, "f%016x", math.Float64bits(x))
	case float32:
		fmt.Fprintf(w, "f%016x", math.Float64bits(float64(x)))
	default:
		r := reflect.ValueOf(v)
		if r.Kind() == reflect.Slice || r.Kind() == reflect.Array {
			fmt.Fprintf(w, "a%d:", r.Len())
			for i := 0; i < r.Len(); i++ {
				value(w, r.Index(i).Interface())
			}
		} else {
			fmt.Fprintf(w, "n%v;", v)
		}
	}
}
