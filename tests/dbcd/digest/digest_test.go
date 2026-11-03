package digest

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"testing"
)

func TestFieldStreamMatchesPinnedOracle(t *testing.T) {
	h := sha256.New()
	Row(h, 7, map[string]any{"ID": uint64(7), "Value": uint64(99)})
	Row(h, 900, map[string]any{"ID": uint64(900), "Value": uint64(77)})
	// Independently emitted by pinned DBCD for fixture/CommonBoth.
	if got := hex.EncodeToString(h.Sum(nil)); got != "1d35240dee6c0e347c515ac05a9d5c0c8e6a186d62632d17aa4c98fb36805aab" {
		t.Fatal(got)
	}
}
func TestFieldStreamIncludesTailAndExactIntegers(t *testing.T) {
	makeDigest := func(change bool) []byte {
		h := sha256.New()
		for id := uint32(1); id <= 300; id++ {
			value := uint64(18446744073709551615)
			if change && id == 201 {
				value--
			}
			Row(h, id, map[string]any{"ID": uint64(id), "Value": value})
		}
		return h.Sum(nil)
	}
	if bytes.Equal(makeDigest(false), makeDigest(true)) {
		t.Fatal("row 201 uint64 change disappeared")
	}
}
