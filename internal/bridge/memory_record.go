package bridge

import (
	"bytes"
	"encoding/binary"
	"errors"
	"hash/adler32"
)

const MemoryHeaderBytes = 80
const MemoryTrailerBytes = 40
const MemoryMaxPayload = 512 << 10

var MemoryMagic = []byte("LYCMEM06")
var memoryTrailer = []byte("LYCEND06")

type MemoryKind byte

const (
	MemoryIdentity     MemoryKind = 1
	MemoryReceipt      MemoryKind = 2
	MemoryBody         MemoryKind = 3
	MemoryConfirmation MemoryKind = 4
	MemoryInputState   MemoryKind = 5
)

// MemoryHeader is a bounded corruption-checked envelope, NOT publication proof.
// The coordinator must correlate a fresh challenge and validate its predicates.
type MemoryHeader struct {
	Nonce    [16]byte
	Runtime  [16]byte
	Ticket   [16]byte
	Kind     MemoryKind
	State    byte
	Sequence uint32
	Length   uint32
	Checksum uint32
}

func EncodeMemoryRecord(h MemoryHeader, payload []byte) ([]byte, error) {
	if len(payload) > MemoryMaxPayload || h.Kind < MemoryIdentity || h.Kind > MemoryInputState || h.State == 0 {
		return nil, errors.New("bridge.memory_record_invalid")
	}
	b := make([]byte, MemoryHeaderBytes+len(payload)+MemoryTrailerBytes)
	copy(b, MemoryMagic)
	copy(b[8:24], h.Nonce[:])
	copy(b[24:40], h.Runtime[:])
	copy(b[40:56], h.Ticket[:])
	b[56] = byte(h.Kind)
	b[57] = h.State
	binary.LittleEndian.PutUint32(b[60:64], h.Sequence)
	binary.LittleEndian.PutUint32(b[64:68], uint32(len(payload)))
	binary.LittleEndian.PutUint32(b[68:72], adler32.Checksum(payload))
	binary.LittleEndian.PutUint32(b[72:76], MemoryHeaderBytes)
	binary.LittleEndian.PutUint32(b[76:80], adler32.Checksum(b[:76]))
	copy(b[80:], payload)
	at := 80 + len(payload)
	copy(b[at:], memoryTrailer)
	copy(b[at+8:], h.Nonce[:])
	copy(b[at+24:], h.Runtime[:])
	return b, nil
}

func DecodeMemoryHeader(b []byte) (MemoryHeader, error) {
	h := MemoryHeader{}
	if len(b) < MemoryHeaderBytes || !bytes.Equal(b[:8], MemoryMagic) || b[58] != 0 || b[59] != 0 || binary.LittleEndian.Uint32(b[72:76]) != MemoryHeaderBytes || binary.LittleEndian.Uint32(b[76:80]) != adler32.Checksum(b[:76]) {
		return h, errors.New("bridge.memory_header_invalid")
	}
	copy(h.Nonce[:], b[8:24])
	copy(h.Runtime[:], b[24:40])
	copy(h.Ticket[:], b[40:56])
	h.Kind = MemoryKind(b[56])
	h.State = b[57]
	h.Sequence = binary.LittleEndian.Uint32(b[60:64])
	h.Length = binary.LittleEndian.Uint32(b[64:68])
	h.Checksum = binary.LittleEndian.Uint32(b[68:72])
	if h.Length > MemoryMaxPayload || h.Kind < MemoryIdentity || h.Kind > MemoryInputState || h.State == 0 {
		return MemoryHeader{}, errors.New("bridge.memory_header_bounds")
	}
	return h, nil
}

func DecodeMemoryRecord(b []byte) (MemoryHeader, []byte, error) {
	h, err := DecodeMemoryHeader(b)
	if err != nil {
		return h, nil, err
	}
	at := MemoryHeaderBytes + int(h.Length)
	if len(b) != at+MemoryTrailerBytes || adler32.Checksum(b[80:at]) != h.Checksum || !bytes.Equal(b[at:at+8], memoryTrailer) || !bytes.Equal(b[at+8:at+24], h.Nonce[:]) || !bytes.Equal(b[at+24:], h.Runtime[:]) {
		return h, nil, errors.New("bridge.memory_body_invalid")
	}
	return h, b[80:at], nil
}
