package memory

import (
	"context"
	"hash/adler32"
	"testing"

	"github.com/follenfang/lycheedev/internal/bridge"
)

func TestBodyRequiresExactHeadAuthorization(t *testing.T) {
	s := source(8192)
	h := bridge.MemoryHeader{Kind: bridge.MemoryBody, State: 3, Sequence: 7}
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

func TestBodyStaticHeaderRejectedBeforePayloadRead(t *testing.T) {
	base := bridge.MemoryHeader{Kind: bridge.MemoryBody, State: 3, Sequence: 7, Nonce: [16]byte{1}, Runtime: [16]byte{2}, Ticket: [16]byte{3}}
	for _, tc := range []struct {
		name  string
		edit  func(*bridge.MemoryHeader)
		valid bool
	}{
		{"reported-body", func(*bridge.MemoryHeader) {}, true},
		{"last-sequence", func(h *bridge.MemoryHeader) { h.Sequence = ^uint32(0) }, true},
		{"wrong-state", func(h *bridge.MemoryHeader) { h.State = 1 }, false},
		{"zero-sequence", func(h *bridge.MemoryHeader) { h.Sequence = 0 }, false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			s := source(8192)
			h := base
			tc.edit(&h)
			payload := []byte(`{"ok":true}`)
			wire, err := bridge.EncodeMemoryRecord(h, payload)
			if err != nil {
				t.Fatal(err)
			}
			copy(s.data[64:], wire)
			selector := Selector{Kind: bridge.MemoryBody, Nonce: h.Nonce, Runtime: h.Runtime, Ticket: h.Ticket, BodyAuthorized: true, BodyLength: uint32(len(payload)), BodyChecksum: adler32.Checksum(payload)}
			_, err = ReadRecord(context.Background(), s, 64, selector)
			if (err == nil) != tc.valid || !tc.valid && s.calls != 1 {
				t.Fatalf("valid=%t reads=%d err=%v", tc.valid, s.calls, err)
			}
		})
	}
}
