package noncelab

import (
	"bytes"
	"encoding/binary"
	"errors"
	"hash/adler32"
)

const HeaderSize = 64
const MaxPayload = 524288

var Magic = []byte("LYCHN001")
var Trailer = []byte("LYCHEND1")

type Identity struct {
	Nonce  [16]byte
	Run    [8]byte
	Epoch  uint32
	Ticket [8]byte
}

func Encode(id Identity, payload []byte, state byte) []byte {
	if len(payload) > MaxPayload {
		panic("payload exceeds lab budget")
	}
	out := make([]byte, HeaderSize+len(payload)+24)
	copy(out, Magic)
	copy(out[8:24], id.Nonce[:])
	copy(out[24:32], id.Run[:])
	binary.LittleEndian.PutUint32(out[32:36], id.Epoch)
	copy(out[36:44], id.Ticket[:])
	out[44] = state
	binary.LittleEndian.PutUint32(out[48:52], HeaderSize)
	binary.LittleEndian.PutUint32(out[52:56], uint32(len(payload)))
	binary.LittleEndian.PutUint32(out[56:60], adler32.Checksum(payload))
	binary.LittleEndian.PutUint32(out[60:64], adler32.Checksum(out[:60]))
	copy(out[64:], payload)
	copy(out[64+len(payload):], Trailer)
	copy(out[72+len(payload):], id.Nonce[:])
	return out
}

// Header validation gates any large payload read. A valid record is not a commit proof.
func ValidateHeader(b []byte, id Identity) (int, error) {
	if len(b) < 64 || !bytes.Equal(b[:8], Magic) || !bytes.Equal(b[8:24], id.Nonce[:]) || !bytes.Equal(b[24:32], id.Run[:]) || binary.LittleEndian.Uint32(b[32:36]) != id.Epoch || !bytes.Equal(b[36:44], id.Ticket[:]) || b[44] != 3 || b[45] != 0 || b[46] != 0 || b[47] != 0 || binary.LittleEndian.Uint32(b[48:52]) != 64 || binary.LittleEndian.Uint32(b[60:64]) != adler32.Checksum(b[:60]) {
		return 0, errors.New("invalid header or identity")
	}
	n := binary.LittleEndian.Uint32(b[52:56])
	if n > MaxPayload {
		return 0, errors.New("payload budget")
	}
	return int(n), nil
}
func Validate(b []byte, id Identity) error {
	n, e := ValidateHeader(b, id)
	if e != nil {
		return e
	}
	if len(b) < 64+n+24 {
		return errors.New("truncated record")
	}
	if binary.LittleEndian.Uint32(b[56:60]) != adler32.Checksum(b[64:64+n]) || !bytes.Equal(b[64+n:72+n], Trailer) || !bytes.Equal(b[72+n:88+n], id.Nonce[:]) {
		return errors.New("invalid payload or trailer")
	}
	return nil
}
