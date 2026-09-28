package bridge

import (
	"bytes"
	"testing"
)

func TestMemoryRecordIntegrityAndBounds(t *testing.T) {
	h := MemoryHeader{Kind: MemoryBody, State: 3, Sequence: 17}
	copy(h.Nonce[:], "nonce-one-unique")
	copy(h.Runtime[:], "runtime-unique")
	copy(h.Ticket[:], "ticket-unique")
	payload := []byte("中文\x00binary")
	b, err := EncodeMemoryRecord(h, payload)
	if err != nil {
		t.Fatal(err)
	}
	actual, decoded, err := DecodeMemoryRecord(b)
	if err != nil || actual.Nonce != h.Nonce || actual.Runtime != h.Runtime || actual.Sequence != 17 || !bytes.Equal(decoded, payload) {
		t.Fatalf("%+v %q %v", actual, decoded, err)
	}
	for _, at := range []int{0, 8, 24, 40, 56, 57, 58, 60, 64, 68, 72, 76, 80, len(b) - 1} {
		bad := append([]byte(nil), b...)
		bad[at] ^= 0x55
		if _, _, err = DecodeMemoryRecord(bad); err == nil {
			t.Fatalf("tamper %d accepted", at)
		}
	}
	if _, _, err = DecodeMemoryRecord(b[:len(b)-1]); err == nil {
		t.Fatal("truncated accepted")
	}
	if _, err = EncodeMemoryRecord(h, make([]byte, MemoryMaxPayload+1)); err == nil {
		t.Fatal("oversized accepted")
	}
}
