package memory

import (
	"bytes"
	"math/rand"
	"testing"
)

func TestSIMDDifferentialBoundariesAndDenseFalseAnchors(t *testing.T) {
	r := rand.New(rand.NewSource(9128))
	for i := 0; i < 12000; i++ {
		data := make([]byte, r.Intn(4097))
		pattern := make([]byte, r.Intn(96))
		_, _ = r.Read(data)
		_, _ = r.Read(pattern)
		if i%3 == 0 { // Dense two-anchor false positives must still verify interior.
			for j := range data {
				data[j] = 'a'
			}
			for j := range pattern {
				pattern[j] = 'a'
			}
			if len(pattern) > 2 {
				pattern[len(pattern)/2] = 'b'
			}
		}
		if len(pattern) <= len(data) && i%2 == 0 {
			at := r.Intn(len(data) - len(pattern) + 1)
			if i%4 == 0 {
				at = len(data) - len(pattern)
			}
			copy(data[at:], pattern)
		}
		if got, want := IndexSIMD(data, pattern), bytes.Index(data, pattern); got != want {
			t.Fatalf("case %d lengths %d/%d: SIMD=%d scalar=%d", i, len(data), len(pattern), got, want)
		}
	}
}
