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
	r, err := ReadRecord(context.Background(), s, 64, selector)
	if err != nil || string(r.Payload) != "result" {
		t.Fatalf("%+v %v", r, err)
	}

	selector.Ticket[0] ^= 1
	if _, err = ReadRecord(context.Background(), s, 64, selector); err == nil {
		t.Fatal("wrong ticket accepted")
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
