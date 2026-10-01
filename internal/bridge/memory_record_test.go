package bridge

import (
	"bytes"
	"encoding/binary"
	"hash/adler32"
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

func TestMemoryRecordRejectsOldWireVersions(t *testing.T) {
	wire, err := EncodeMemoryRecord(MemoryHeader{Kind: MemoryIdentity, State: 1}, []byte("{}"))
	if err != nil {
		t.Fatal(err)
	}
	if string(wire[:8]) != "LYCMEM06" || string(wire[82:90]) != "LYCEND06" {
		t.Fatal("wrong current version")
	}
	for _, version := range []string{"04", "05"} {
		t.Run("header"+version, func(t *testing.T) {
			old := append([]byte(nil), wire...)
			copy(old[:8], "LYCMEM"+version)
			binary.LittleEndian.PutUint32(old[76:80], adler32.Checksum(old[:76]))
			if _, _, err := DecodeMemoryRecord(old); err == nil {
				t.Fatal("valid-checksum old header accepted")
			}
		})
		t.Run("trailer"+version, func(t *testing.T) {
			old := append([]byte(nil), wire...)
			copy(old[82:90], "LYCEND"+version)
			if _, _, err := DecodeMemoryRecord(old); err == nil {
				t.Fatal("old trailer accepted")
			}
		})
	}
}
