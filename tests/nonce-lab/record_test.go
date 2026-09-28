package noncelab

import (
	"bytes"
	"encoding/hex"
	"os"
	"testing"
)

func identity() Identity {
	var id Identity
	n, _ := hex.DecodeString("00112233445566778899aabbccddeeff")
	copy(id.Nonce[:], n)
	copy(id.Run[:], "RUN00001")
	id.Epoch = 1
	copy(id.Ticket[:], "TICKET01")
	return id
}
func TestLuaFixture(t *testing.T) {
	path := os.Getenv("NONCE_LAB_FIXTURE")
	if path == "" {
		t.Skip("explicit cross-language fixture path required")
	}
	b, e := os.ReadFile(path)
	if e != nil {
		t.Fatal(e)
	}
	if !bytes.Equal(b, Encode(identity(), []byte("hello\x00world"), 3)) {
		t.Fatal("Lua/Go encoding mismatch")
	}
}
func TestBoundaries(t *testing.T) {
	id := identity()
	for offset := 48; offset < 80; offset++ {
		data := make([]byte, 256)
		copy(data[offset:], Encode(id, []byte("hello"), 3))
		r := []Region{{data, true, true, true}}
		s := Scan(r, id.Nonce[:], 8, 64)
		valid, _ := VerifyCandidates(r, s.Hits, id, true)
		if len(valid) != 1 || valid[0].Offset != offset {
			t.Fatalf("offset %d: %v", offset, valid)
		}
	}
}
func TestStaleAndCorrupt(t *testing.T) {
	id := identity()
	for _, kind := range []string{"run", "epoch", "ticket", "nonce", "ack", "checksum", "trailer", "short", "invalid-address"} {
		t.Run(kind, func(t *testing.T) {
			other := id
			state := byte(3)
			switch kind {
			case "run":
				other.Run[0]++
			case "epoch":
				other.Epoch++
			case "ticket":
				other.Ticket[0]++
			case "nonce":
				other.Nonce[0]++
			case "ack":
				state = 4
			}
			b := Encode(other, []byte("identical payload"), state)
			switch kind {
			case "checksum":
				b[64] ^= 1
			case "trailer":
				b[len(b)-24] ^= 1
			case "short":
				b = b[:65]
			case "invalid-address":
				b = nil
			}
			if Validate(b, id) == nil {
				t.Fatal("invalid record accepted")
			}
		})
	}
}
func TestRegionsAndGap(t *testing.T) {
	id := identity()
	p := id.Nonce[:]
	r := []Region{{p[:8], true, true, true}, {p[8:], true, true, true}, {p, false, true, true}, {p, true, false, true}, {p, true, true, false}}
	s := Scan(r, p, 8, 64)
	if len(s.Hits) != 0 {
		t.Fatal("stitched across gap or scanned excluded region")
	}
	var planned uint64
	for _, w := range s.Workers {
		planned += w.PlannedBytes
	}
	if planned != 16 {
		t.Fatal(planned)
	}
}
func TestOnlyAssociatedPayload(t *testing.T) {
	id := identity()
	old := id
	old.Ticket[0]++
	data := append(Encode(old, make([]byte, MaxPayload), 3), Encode(id, []byte("small"), 3)...)
	r := []Region{{data, true, true, true}}
	s := Scan(r, id.Nonce[:], 8, 4096)
	valid, n := VerifyCandidates(r, s.Hits, id, true)
	if len(valid) != 1 || n != 5 {
		t.Fatalf("read unassociated payload: %d %v", n, valid)
	}
}

func TestAllScanEnginesAgree(t *testing.T) {
	id := identity()
	data := make([]byte, 16384)
	for _, off := range []int{0, 1009, 2041, 4000, 8190} {
		copy(data[off:], Encode(id, []byte("test"), 3))
	}
	r := []Region{{data, true, true, true}}
	for _, workers := range []int{1, 8} {
		for _, simd := range []bool{false, true} {
			for _, buffered := range []bool{false, true} {
				s := ScanBuffered(r, id.Nonce[:], workers, 1024, simd, buffered)
				valid, n := VerifyCandidates(r, s.Hits, id, true)
				if len(valid) != 5 || n != 20 {
					t.Fatalf("workers=%d simd=%v buffered=%v: %v", workers, simd, buffered, valid)
				}
				var scanned uint64
				for _, w := range s.Workers {
					scanned += w.ScannedBytes
					if !w.Complete || w.Truncated {
						t.Fatal(w)
					}
				}
				if scanned != uint64(len(data)) {
					t.Fatal("overlap double counted", scanned)
				}
			}
		}
	}
}
