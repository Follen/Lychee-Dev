package memory

import (
	"bytes"
	"context"
	"errors"
	"sync"

	"github.com/follenfang/lycheedev/internal/bridge"
)

type Selector struct {
	// Accept filters bounded decoded candidates before an early lookup return.
	// In particular, a retained prepared HEAD must not hide its reported HEAD.
	Accept  func(Record) bool
	Nonce   [16]byte
	Runtime [16]byte
	Ticket  [16]byte
	Kind    bridge.MemoryKind
	// Body reads require the length/checksum obtained from a verified HEAD.
	BodyLength     uint32
	BodyChecksum   uint32
	BodyAuthorized bool
}
type Record struct {
	Address uint64              `json:"address,string"`
	Header  bridge.MemoryHeader `json:"header"`
	Payload []byte              `json:"payload"`
}

func (s Selector) matches(h bridge.MemoryHeader) bool {
	zero := [16]byte{}
	if s.Kind != 0 && h.Kind != s.Kind || s.Nonce != zero && h.Nonce != s.Nonce || s.Runtime != zero && h.Runtime != s.Runtime || s.Ticket != zero && h.Ticket != s.Ticket {
		return false
	}
	if h.Kind == bridge.MemoryBody {
		return s.BodyAuthorized && h.Length == s.BodyLength && h.Checksum == s.BodyChecksum && s.Nonce != zero && s.Runtime != zero && s.Ticket != zero
	}
	return h.Length <= 16<<10
}

// ReadRecord gates allocation behind fixed-header identity checks. The second
// read includes that header and must match, preventing header/body torn reads.
func ReadRecord(ctx context.Context, src Source, address uint64, selector Selector) (Record, error) {
	head := make([]byte, bridge.MemoryHeaderBytes)
	n, err := src.Read(ctx, address, head)
	if err != nil || n != len(head) {
		return Record{}, errors.New("memory.header_unavailable")
	}
	h, err := bridge.DecodeMemoryHeader(head)
	if err != nil {
		return Record{}, err
	}
	if !selector.matches(h) {
		return Record{}, errors.New("memory.record_mismatch")
	}
	data := make([]byte, bridge.MemoryHeaderBytes+int(h.Length)+bridge.MemoryTrailerBytes)
	n, err = src.Read(ctx, address, data)
	if err != nil || n != len(data) {
		return Record{}, errors.New("memory.body_unavailable")
	}
	if !bytes.Equal(head, data[:len(head)]) {
		return Record{}, errors.New("memory.record_changed")
	}
	h, payload, err := bridge.DecodeMemoryRecord(data)
	if err != nil {
		return Record{}, err
	}
	record := Record{address, h, append([]byte(nil), payload...)}
	if selector.Accept != nil && !selector.Accept(record) {
		return Record{}, errors.New("memory.predicate_mismatch")
	}
	return record, nil
}

// Lookup discovers candidates only. A matching envelope is not proof of the
// predicate carried inside; the lifecycle validates fresh response semantics.
func Lookup(ctx context.Context, src Source, selector Selector, opts Options, first bool) ([]Record, Coverage, error) {
	pattern := bridge.MemoryMagic
	offset := uint64(0)
	if selector.Nonce != ([16]byte{}) {
		pattern = selector.Nonce[:]
		offset = 8
	}
	var mu sync.Mutex
	records := []Record{}
	seen := map[uint64]bool{}
	limit := opts.MaxHits
	if limit == 0 {
		limit = 4096
	}
	opts.Visit = func(hit Hit) bool {
		if hit.Address < offset {
			return false
		}
		address := hit.Address - offset
		record, err := ReadRecord(ctx, src, address, selector)
		if err != nil {
			return false
		}
		mu.Lock()
		defer mu.Unlock()
		if !seen[address] {
			if len(records) >= limit {
				return true
			}
			seen[address] = true
			records = append(records, record)
		}
		return first
	}
	result, err := Scan(ctx, src, [][]byte{pattern}, opts)
	return records, result.Coverage, err
}
