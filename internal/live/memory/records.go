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
	Accept func(Record) bool
	// RefreshRejected may schedule bounded INPUT reads after a CRC-valid
	// predicate rejection. Its returned address is untrusted and independently
	// read with this selector. A miss resumes the original scan traversal.
	RefreshRejected func(context.Context, Source, Record) (uint64, error)
	RefreshBudget   *InputRefreshBudget
	Nonce           [16]byte
	Runtime         [16]byte
	Ticket          [16]byte
	Kind            bridge.MemoryKind
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
func ReadRecord(ctx context.Context, src Source, address uint64, selector Selector) (record Record, err error) {
	record, err = readRecord(ctx, src, address, selector)
	if err != nil {
		return Record{}, err
	}
	return record, nil
}

var errPredicateMismatch = errors.New("memory.predicate_mismatch")

// Only lookup sees a rejected record, after immutable-header and CRC checks.
func readRecord(ctx context.Context, src Source, address uint64, selector Selector) (record Record, err error) {
	src, session := measured(src, nil)
	if !session.candidate(false) {
		return Record{}, ErrBudget
	}
	defer func() {
		session.update(func(st *Stats) {
			if err == nil {
				st.Validated++
			} else {
				st.Rejected++
			}
		})
	}()
	head := make([]byte, bridge.MemoryHeaderBytes)
	n, err := src.Read(ctx, address, head)
	if err != nil || n != len(head) {
		if errors.Is(err, ErrBudget) || errors.Is(err, ErrSessionOwnership) || ctx.Err() != nil {
			return Record{}, errors.Join(err, ctx.Err())
		}
		return Record{}, errors.New("memory.header_unavailable")
	}
	h, err := bridge.DecodeMemoryHeader(head)
	if err != nil {
		session.update(func(st *Stats) { st.HeaderRejected++ })
		return Record{}, err
	}
	if !selector.matches(h) {
		session.update(func(st *Stats) { st.HeaderRejected++ })
		return Record{}, errors.New("memory.record_mismatch")
	}
	data := make([]byte, bridge.MemoryHeaderBytes+int(h.Length)+bridge.MemoryTrailerBytes)
	n, err = src.Read(ctx, address, data)
	if err != nil || n != len(data) {
		if errors.Is(err, ErrBudget) || errors.Is(err, ErrSessionOwnership) || ctx.Err() != nil {
			return Record{}, errors.Join(err, ctx.Err())
		}
		return Record{}, errors.New("memory.body_unavailable")
	}
	if !bytes.Equal(head, data[:len(head)]) {
		session.update(func(st *Stats) { st.IntegrityRejected++ })
		return Record{}, errors.New("memory.record_changed")
	}
	h, payload, err := bridge.DecodeMemoryRecord(data)
	if err != nil {
		session.update(func(st *Stats) { st.IntegrityRejected++ })
		return Record{}, err
	}
	record = Record{address, h, append([]byte(nil), payload...)}
	if selector.Accept != nil && !selector.Accept(record) {
		session.update(func(st *Stats) { st.PredicateRejected++ })
		return record, errPredicateMismatch
	}
	return record, nil
}

// Lookup discovers candidates only. A matching envelope is not proof of the
// predicate carried inside; the lifecycle validates fresh response semantics.
func Lookup(ctx context.Context, src Source, selector Selector, opts Options, first bool) ([]Record, Coverage, error) {
	return lookup(ctx, src, selector, opts, first, nil)
}

func lookup(ctx context.Context, src Source, selector Selector, opts Options, first bool, hints *Hints) ([]Record, Coverage, error) {
	src, session := measured(src, opts.Session)
	pattern := bridge.MemoryMagic
	offset := uint64(0)
	if selector.Nonce != ([16]byte{}) {
		pattern = selector.Nonce[:]
		offset = 8
	}
	var mu sync.Mutex
	records := []Record{}
	seen := map[uint64]bool{}
	var refreshErr error
	var inputTiming *InputLookupTiming
	if selector.Kind == bridge.MemoryInputState && first {
		opts.acceptanceObserver = func(offset, drain int64) {
			inputTiming = &InputLookupTiming{AcceptedAtOffsetMillis: offset, DrainMillis: drain}
		}
	}
	limit := opts.MaxHits
	if limit == 0 {
		limit = 4096
	}
	// Learning validates complete small records already in the scan buffer.
	// Hints never retain payloads or authorize BODY reads and are bounded at 64 entries.
	opts.Visit = nil
	if hints != nil {
		opts.learningPattern = bridge.MemoryMagic
		opts.learn = func(hit Hit, view []byte, base uint64) {
			at := hit.Address - base
			if at > uint64(len(view)) || uint64(len(view))-at < bridge.MemoryHeaderBytes {
				return
			}
			header, err := bridge.DecodeMemoryHeader(view[at:])
			if err != nil || header.Kind == bridge.MemoryBody || header.Length > 16<<10 || (selector.Runtime != ([16]byte{}) && selector.Runtime != header.Runtime) {
				return
			}
			end := at + bridge.MemoryHeaderBytes + uint64(header.Length) + bridge.MemoryTrailerBytes
			if end > uint64(len(view)) || (header.Kind == bridge.MemoryInputState && !hints.inputHintUseful(hit.Address, header)) {
				return
			}
			if _, _, err := bridge.DecodeMemoryRecord(view[at:end]); err != nil {
				return
			}
			hints.learn(Record{Address: hit.Address, Header: header})
			session.update(func(st *Stats) { st.Learned++ })
		}
	}
	opts.candidate = func(refreshCtx context.Context, hit Hit, view []byte, base uint64) bool {
		if hit.Address < offset {
			return false
		}
		address := hit.Address - offset
		if address >= base && address-base <= uint64(len(view)) && uint64(len(view))-(address-base) >= bridge.MemoryHeaderBytes {
			header, err := bridge.DecodeMemoryHeader(view[address-base:])
			if err != nil {
				session.update(func(st *Stats) { st.HeaderRejected++ })
				return false
			}
			if !selector.matches(header) {
				session.update(func(st *Stats) { st.HeaderRejected++ })
				return false
			}
		}
		record, err := readRecord(ctx, src, address, selector)
		if errors.Is(err, errPredicateMismatch) && selector.RefreshRejected != nil && selector.RefreshBudget != nil && inputRefreshEligible(selector) {
			bounded := selector.RefreshBudget.Source(src)
			refreshed, localErr := selector.RefreshRejected(refreshCtx, bounded, record)
			if localErr == nil && refreshed != 0 {
				refreshCtx, scopeErr := inputRefreshContext(bounded)
				if scopeErr == nil {
					record, scopeErr = ReadRecord(refreshCtx, bounded, refreshed, selector)
					scopeErr = errors.Join(scopeErr, refreshCtx.Err())
					if scopeErr == nil {
						address, err = record.Address, nil
					}
				}
				// A changed/unavailable returned record is another miss. Only
				// shared quota/ownership failures escape the original traversal.
				if errors.Is(scopeErr, ErrBudget) || errors.Is(scopeErr, ErrSessionOwnership) {
					localErr = scopeErr
				}
			}
			EndInputRefresh(bounded)
			if localErr != nil && !errors.Is(localErr, ErrNearbyBudget) && !errors.Is(localErr, context.DeadlineExceeded) && !errors.Is(localErr, context.Canceled) && !errors.Is(localErr, errPredicateMismatch) {
				mu.Lock()
				refreshErr = errors.Join(refreshErr, localErr)
				mu.Unlock()
			}
		}
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
	result.Coverage.InputTiming = inputTiming
	if refreshErr != nil {
		return nil, result.Coverage, errors.Join(err, refreshErr)
	}
	return records, result.Coverage, err
}

func inputRefreshEligible(s Selector) bool {
	zero := [16]byte{}
	return s.Kind == bridge.MemoryInputState && s.Accept != nil && !s.BodyAuthorized && s.Runtime != zero && s.Nonce == s.Runtime && s.Ticket == zero
}
