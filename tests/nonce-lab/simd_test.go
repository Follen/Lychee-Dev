package noncelab

import (
	"bytes"
	"math/rand"
	"testing"
)

func TestSIMDDifferential(t *testing.T) {
	r := rand.New(rand.NewSource(42))
	for n := 0; n < 4096; n++ {
		b := make([]byte, n)
		r.Read(b)
		for _, size := range []int{0, 1, 2, 8, 16, 33, 64} {
			p := make([]byte, size)
			r.Read(p)
			if n >= size && n%2 == 0 {
				copy(b[n-size:], p)
			}
			if got, want := IndexSIMD(b, p), bytes.Index(b, p); got != want {
				t.Fatalf("size=%d n=%d got=%d want=%d", size, n, got, want)
			}
		}
	}
}
func TestSIMDDenseAnchors(t *testing.T) {
	for n := 0; n < 256; n++ {
		b := bytes.Repeat([]byte{'a'}, n)
		p := []byte("abaa")
		if n > 8 {
			copy(b[n-4:], p)
		}
		if IndexSIMD(b, p) != bytes.Index(b, p) {
			t.Fatal(n)
		}
	}
}
