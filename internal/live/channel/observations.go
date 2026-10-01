package channel

import (
	"context"
	"encoding/json"
	"errors"
	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/live/memory"
	"hash/adler32"
)

type ObservationQuery struct {
	Kind     string
	Envelope bridge.SlotEnvelope
	Identity Identity
}
type Observation struct {
	Receipt         Receipt
	Payload         []byte
	InputDiagnostic *InputDiagnostic
}
type RecordReader interface {
	Find(context.Context, memory.Selector, bool) (memory.LookupResult, error)
}

// ObserveRecords is the transport's shared validator for native and simulated
// adapters. Domain callers never receive addresses or partially decoded bodies.
func ObserveRecords(ctx context.Context, reader RecordReader, q ObservationQuery) (Observation, error) {
	e, i := q.Envelope, q.Identity
	if e.Schema == "" {
		e.Schema = bridge.SlotSchema
	}
	nonce, _ := tokenBytes(e.Nonce)
	runtime, _ := tokenBytes(e.Runtime)
	ticket, _ := tokenBytes(e.Ticket)
	selector := memory.Selector{Nonce: nonce, Runtime: runtime, Ticket: ticket, Kind: bridge.MemoryReceipt}
	first := true
	switch q.Kind {
	case "receipt":
		if e.Action == "confirm" {
			selector.Kind = bridge.MemoryConfirmation
		}
	case "result":
		selector.Accept = func(record memory.Record) bool {
			r, err := decodePublicationReceipt(record)
			return err == nil && r.Schema == e.Schema && r.State == "reported" && r.Action == "prepare" && r.Nonce == e.Nonce && r.Ticket == e.Ticket && r.Runtime == e.Runtime && r.Owner == e.Owner && r.Fence == e.Fence && r.GUID == e.GUID && r.Build == e.Build && r.Product == i.Product && r.Release == i.Release
		}
	case "bootstrap_changed":
		first = false
		selector.Runtime = [16]byte{}
		selector.Accept = func(record memory.Record) bool {
			r, err := decodePublicationReceipt(record)
			if err != nil {
				return false
			}
			return r.Schema == e.Schema && r.Nonce == e.Nonce && r.Ticket == e.Ticket && r.Action == "bind" && r.State == "rejected" && r.Reason == "slot_runtime_changed" && r.Runtime != e.Runtime && r.GUID == e.GUID && r.Build == e.Build && r.Release == i.Release && r.NextSlot == e.Index+1 && r.Owner == ""
		}
	default:
		return Observation{}, errors.New("live.channel_observation_invalid")
	}
	found, err := reader.Find(ctx, selector, first)
	if err != nil {
		return Observation{}, err
	}
	if len(found.Records) == 0 {
		return Observation{}, ErrPending
	}
	var r Receipt
	if q.Kind == "receipt" {
		r, err = decodeReceipt(found.Records[0], e)
		var rejection *RejectedError
		if err != nil && !errors.As(err, &rejection) {
			return Observation{}, err
		}
		if r.Product != i.Product || r.Release != i.Release {
			return Observation{}, errors.New("live.channel_receipt_target_mismatch")
		}
		return Observation{Receipt: r}, err
	}
	// Simulated adapters share the same gate; do not rely on them applying Accept.
	if selector.Accept == nil || !selector.Accept(found.Records[0]) {
		return Observation{}, errors.New("live.channel_receipt_mismatch")
	}
	if r, err = decodePublicationReceipt(found.Records[0]); err != nil {
		return Observation{}, err
	}
	if q.Kind == "bootstrap_changed" {
		for _, record := range found.Records[1:] {
			other, decodeErr := decodePublicationReceipt(record)
			if decodeErr != nil || !selector.Accept(record) || other.Runtime != r.Runtime {
				return Observation{}, errors.New("live.channel_runtime_ambiguous")
			}
		}
		return Observation{Receipt: r}, nil
	}
	if r.ReportBytes == 0 || r.ReportBytes > bridge.MemoryMaxPayload {
		return Observation{}, errors.New("live.channel_report_length")
	}
	body, err := reader.Find(ctx, memory.Selector{Nonce: nonce, Runtime: runtime, Ticket: ticket, Kind: bridge.MemoryBody, BodyAuthorized: true, BodyLength: r.ReportBytes, BodyChecksum: r.ReportChecksum}, true)
	if err != nil {
		return Observation{}, err
	}
	if len(body.Records) == 0 {
		return Observation{}, ErrPending
	}
	// ReadRecord qualifies BODY before allocation. Repeat the returned-record
	// contract here for adapters, without assuming HEAD and BODY sequences equal.
	record := body.Records[0]
	h := record.Header
	if h.Kind != bridge.MemoryBody || h.State != 3 || h.Sequence == 0 || nonce == [16]byte{} || runtime == [16]byte{} || ticket == [16]byte{} || h.Nonce != nonce || h.Runtime != runtime || h.Ticket != ticket || h.Length != r.ReportBytes || h.Checksum != r.ReportChecksum || len(record.Payload) != int(r.ReportBytes) || adler32.Checksum(record.Payload) != r.ReportChecksum {
		return Observation{}, errors.New("live.channel_report_record_invalid")
	}
	if !json.Valid(record.Payload) {
		return Observation{}, errors.New("live.channel_report_json")
	}
	return Observation{Receipt: r, Payload: record.Payload}, nil
}
