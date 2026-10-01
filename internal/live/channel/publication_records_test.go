package channel

import (
	"context"
	"encoding/json"
	"errors"
	"hash/adler32"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/live/memory"
)

// Use the wire codec, including its checksum, rather than payload-only records.
func publicationTestRecord(t *testing.T, r Receipt, h bridge.MemoryHeader) memory.Record {
	t.Helper()
	payload, err := json.Marshal(r)
	if err != nil {
		t.Fatal(err)
	}
	wire, err := bridge.EncodeMemoryRecord(h, payload)
	if err != nil {
		t.Fatal(err)
	}
	header, payload, err := bridge.DecodeMemoryRecord(wire)
	if err != nil {
		t.Fatal(err)
	}
	return memory.Record{Header: header, Payload: payload}
}

func publicationTestReceipt() (Receipt, bridge.SlotEnvelope, bridge.MemoryHeader) {
	i := Identity{Schema: bridge.SlotSchema, Runtime: strings.Repeat("1", 32), Slots: bridge.SlotCount, NextSlot: 2, GUID: "g", Character: "c", Realm: "r", Build: "b", Product: "retail", Release: "3.1.0"}
	e := bridge.SlotEnvelope{Schema: bridge.SlotSchema, Index: 1, Runtime: i.Runtime, GUID: i.GUID, Build: i.Build, Nonce: strings.Repeat("2", 32), Ticket: strings.Repeat("0", 32), Action: "bind"}
	r := Receipt{Identity: i, Nonce: e.Nonce, Ticket: e.Ticket, Action: e.Action, State: "bound"}
	runtime, _ := tokenBytes(r.Runtime)
	nonce, _ := tokenBytes(r.Nonce)
	ticket, _ := tokenBytes(r.Ticket)
	h := bridge.MemoryHeader{Kind: bridge.MemoryReceipt, State: 1, Sequence: 1, Runtime: runtime, Nonce: nonce, Ticket: ticket}
	return r, e, h
}

func TestReceiptPublicationRejectsMalformedChecksummedHeaders(t *testing.T) {
	base, e, head := publicationTestReceipt()
	for _, tc := range []struct {
		name  string
		edit  func(*bridge.MemoryHeader, *Receipt)
		valid bool
	}{
		{"zero-ticket-bind", func(*bridge.MemoryHeader, *Receipt) {}, true},
		{"last-sequence", func(h *bridge.MemoryHeader, _ *Receipt) { h.Sequence = ^uint32(0) }, true},
		{"confirmation", func(h *bridge.MemoryHeader, r *Receipt) { h.Kind = bridge.MemoryConfirmation; r.Action = "confirm" }, true},
		{"wrong-kind", func(h *bridge.MemoryHeader, _ *Receipt) { h.Kind = bridge.MemoryIdentity }, false},
		{"wrong-state", func(h *bridge.MemoryHeader, _ *Receipt) { h.State = 3 }, false},
		{"zero-sequence", func(h *bridge.MemoryHeader, _ *Receipt) { h.Sequence = 0 }, false},
		{"zero-nonce", func(h *bridge.MemoryHeader, r *Receipt) { h.Nonce = [16]byte{}; r.Nonce = strings.Repeat("0", 32) }, false},
		{"zero-runtime", func(h *bridge.MemoryHeader, r *Receipt) { h.Runtime = [16]byte{}; r.Runtime = strings.Repeat("0", 32) }, false},
		{"wrong-nonce", func(h *bridge.MemoryHeader, _ *Receipt) { h.Nonce[0] ^= 1 }, false},
		{"wrong-runtime", func(h *bridge.MemoryHeader, _ *Receipt) { h.Runtime[0] ^= 1 }, false},
		{"wrong-zero-ticket", func(h *bridge.MemoryHeader, _ *Receipt) { h.Ticket[0] = 1 }, false},
		{"confirmation-bind", func(h *bridge.MemoryHeader, _ *Receipt) { h.Kind = bridge.MemoryConfirmation }, false},
		{"receipt-confirm", func(_ *bridge.MemoryHeader, r *Receipt) { r.Action = "confirm" }, false},
		{"confirm-rejection-receipt", func(_ *bridge.MemoryHeader, r *Receipt) {
			r.Action = "confirm"
			r.State = "rejected"
			r.Reason = "slot_target_changed"
		}, true},
	} {
		t.Run(tc.name, func(t *testing.T) {
			h, r, target := head, base, e
			tc.edit(&h, &r)
			target.Nonce, target.Runtime, target.Action = r.Nonce, r.Runtime, r.Action
			_, err := decodeReceipt(publicationTestRecord(t, r, h), target)
			var rejected *RejectedError
			if (err == nil || errors.As(err, &rejected)) != tc.valid {
				t.Fatalf("qualification valid=%t err=%v", tc.valid, err)
			}
		})
	}
}

type publicationResultReader struct{ head, body memory.Record }

func (p publicationResultReader) Find(_ context.Context, s memory.Selector, _ bool) (memory.LookupResult, error) {
	if s.Kind == bridge.MemoryBody {
		return memory.LookupResult{Records: []memory.Record{p.body}}, nil
	}
	return memory.LookupResult{Records: []memory.Record{p.head}}, nil
}

func TestObserveRecordsBodyMatchesQualifiedHead(t *testing.T) {
	r, e, h := publicationTestReceipt()
	r.Action, r.State = "prepare", "reported"
	r.Ticket, e.Ticket = strings.Repeat("3", 32), strings.Repeat("3", 32)
	h.Ticket, _ = tokenBytes(r.Ticket)
	h.Sequence = 11 // A later reported HEAD; BODY retains prepare sequence 7.
	payload := []byte(`{"ok":true}`)
	r.ReportBytes, r.ReportChecksum = uint32(len(payload)), adler32.Checksum(payload)
	head := publicationTestRecord(t, r, h)
	bodyHeader := h
	bodyHeader.Kind = bridge.MemoryBody
	bodyHeader.State = 3
	bodyHeader.Sequence = 7
	for _, tc := range []struct {
		name  string
		edit  func(*bridge.MemoryHeader)
		valid bool
	}{
		{"different-valid-sequence", func(*bridge.MemoryHeader) {}, true},
		{"wrong-kind", func(h *bridge.MemoryHeader) { h.Kind = bridge.MemoryReceipt }, false},
		{"wrong-state", func(h *bridge.MemoryHeader) { h.State = 1 }, false},
		{"zero-sequence", func(h *bridge.MemoryHeader) { h.Sequence = 0 }, false},
		{"other-nonce", func(h *bridge.MemoryHeader) { h.Nonce[0] ^= 1 }, false},
		{"other-ticket", func(h *bridge.MemoryHeader) { h.Ticket[0] ^= 1 }, false},
		{"other-runtime", func(h *bridge.MemoryHeader) { h.Runtime[0] ^= 1 }, false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			h := bodyHeader
			tc.edit(&h)
			wire, err := bridge.EncodeMemoryRecord(h, payload)
			if err != nil {
				t.Fatal(err)
			}
			h, p, err := bridge.DecodeMemoryRecord(wire)
			if err != nil {
				t.Fatal(err)
			}
			reader := publicationResultReader{head: head, body: memory.Record{Header: h, Payload: p}}
			got, err := ObserveRecords(context.Background(), reader, ObservationQuery{Kind: "result", Envelope: e, Identity: r.Identity})
			if (err == nil) != tc.valid || tc.valid && string(got.Payload) != string(payload) {
				t.Fatalf("valid=%t err=%v", tc.valid, err)
			}
		})
	}
}

func TestReceiptPublicationPayloadBound(t *testing.T) {
	r, e, h := publicationTestReceipt()
	payload, _ := json.Marshal(r)
	for _, length := range []int{16384, 16385} {
		p := append(append([]byte(nil), payload...), []byte(strings.Repeat(" ", length-len(payload)))...)
		wire, err := bridge.EncodeMemoryRecord(h, p)
		if err != nil {
			t.Fatal(err)
		}
		h, p, err := bridge.DecodeMemoryRecord(wire)
		if err != nil {
			t.Fatal(err)
		}
		_, err = decodeReceipt(memory.Record{Header: h, Payload: p}, e)
		if (err == nil) != (length == 16384) {
			t.Fatalf("length=%d err=%v", length, err)
		}
	}
}

type publicationTestReader struct {
	record        memory.Record
	finds, bodies int
}

func (p *publicationTestReader) Find(_ context.Context, s memory.Selector, _ bool) (memory.LookupResult, error) {
	p.finds++
	if s.Kind == bridge.MemoryBody {
		p.bodies++
		return memory.LookupResult{}, errors.New("BODY must not be read")
	}
	if s.Accept != nil && !s.Accept(p.record) {
		return memory.LookupResult{}, nil
	}
	return memory.LookupResult{Records: []memory.Record{p.record}}, nil
}

func TestReportedHeadQualifiedBeforeBodyRead(t *testing.T) {
	r, e, h := publicationTestReceipt()
	r.Action, r.State, r.ReportBytes = "prepare", "reported", 2
	e.Action = r.Action
	for _, edit := range []func(*bridge.MemoryHeader){
		func(h *bridge.MemoryHeader) { h.State = 3 },
		func(h *bridge.MemoryHeader) { h.Sequence = 0 },
		func(h *bridge.MemoryHeader) { h.Kind = bridge.MemoryConfirmation },
		func(h *bridge.MemoryHeader) { h.Nonce[0] ^= 1 },
		func(h *bridge.MemoryHeader) { h.Ticket[0] ^= 1 },
	} {
		bad := h
		edit(&bad)
		reader := &publicationTestReader{record: publicationTestRecord(t, r, bad)}
		if _, err := ObserveRecords(context.Background(), reader, ObservationQuery{Kind: "result", Envelope: e, Identity: r.Identity}); !errors.Is(err, ErrPending) || reader.bodies != 0 {
			t.Fatalf("malformed HEAD reached BODY: err=%v bodies=%d", err, reader.bodies)
		}
	}
}

func TestBootstrapChangedRejectUsesTypedReceipt(t *testing.T) {
	r, e, h := publicationTestReceipt()
	r.Runtime = strings.Repeat("4", 32)
	r.State, r.Reason = "rejected", "slot_runtime_changed"
	h.Runtime, _ = tokenBytes(r.Runtime)
	for _, kind := range []bridge.MemoryKind{bridge.MemoryReceipt, bridge.MemoryConfirmation, bridge.MemoryIdentity} {
		h.Kind = kind
		reader := &publicationTestReader{record: publicationTestRecord(t, r, h)}
		_, err := ObserveRecords(context.Background(), reader, ObservationQuery{Kind: "bootstrap_changed", Envelope: e, Identity: r.Identity})
		if (err == nil) != (kind == bridge.MemoryReceipt) {
			t.Fatalf("kind %d: %v", kind, err)
		}
	}
}
