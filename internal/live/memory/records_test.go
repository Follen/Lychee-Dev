package memory

import (
	"context"
	"hash/adler32"
	"testing"

	"github.com/follenfang/lycheedev/internal/bridge"
)

func TestBodyRequiresExactHeadAuthorization(t *testing.T) {
	s := source(8192)
	h := bridge.MemoryHeader{Kind: bridge.MemoryBody, State: 3}
	copy(h.Nonce[:], "current-nonce")
	copy(h.Runtime[:], "current-runtime")
	copy(h.Ticket[:], "current-ticket")
	payload := []byte("result")
	b, _ := bridge.EncodeMemoryRecord(h, payload)
	copy(s.data[64:], b)
	selector := Selector{Nonce: h.Nonce, Runtime: h.Runtime, Ticket: h.Ticket, Kind: bridge.MemoryBody}
	if _, err := ReadRecord(context.Background(), s, 64, selector); err == nil {
		t.Fatal("unauthorized BODY read")
	}
	selector.BodyAuthorized = true
	selector.BodyLength = uint32(len(payload))
	selector.BodyChecksum = adler32.Checksum(payload)
	r, c, err := Lookup(context.Background(), s, selector, Options{ChunkBytes: 64, Workers: 1}, true)
	if err != nil || len(r) != 1 || string(r[0].Payload) != "result" {
		t.Fatalf("%+v %v", r, err)
	}
	if c.Complete {
		t.Fatal("early lookup called full coverage")
	}
	selector.Ticket[0] ^= 1
	if _, err = ReadRecord(context.Background(), s, 64, selector); err == nil {
		t.Fatal("wrong ticket accepted")
	}
}

func TestLookupStreamsPastRejectedOldTelemetry(t *testing.T) {
	s := source(16384)
	h := bridge.MemoryHeader{Kind: bridge.MemoryInputState, State: 1}
	copy(h.Nonce[:], "runtime-nonce")
	old, _ := bridge.EncodeMemoryRecord(h, []byte("stale"))
	for i := 0; i < 30; i++ {
		copy(s.data[i*256:], old)
	}
	fresh, _ := bridge.EncodeMemoryRecord(h, []byte("current"))
	copy(s.data[15000:], fresh)
	selector := Selector{Nonce: h.Nonce, Kind: h.Kind, Accept: func(r Record) bool { return string(r.Payload) == "current" }}
	r, _, err := Lookup(context.Background(), s, selector, Options{MaxHits: 2, Workers: 1, ChunkBytes: 256}, true)
	if err != nil || len(r) != 1 || r[0].Address != 15000 {
		t.Fatalf("fresh record hidden behind rejected anchors: %v %v", r, err)
	}
	selector.Accept = nil
	r, c, err := Lookup(context.Background(), s, selector, Options{MaxHits: 2, Workers: 1, ChunkBytes: 256}, false)
	if err != nil || len(r) != 2 || c.Complete || !c.Truncated {
		t.Fatalf("accepted result cap was lost: %v %+v %v", r, c, err)
	}
}
