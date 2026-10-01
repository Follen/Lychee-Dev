package channel

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/live/memory"
)

const InputDiagnosticSchema = "lycheedev.input-attempt.v1"

// InputDiagnostic describes a callback, not an execution receipt or permission
// to repeat input. SampleMillis is copied from the containing current INPUT.
type InputDiagnostic struct {
	Schema       string `json:"schema"`
	Runtime      string `json:"runtime"`
	AttemptSeq   uint64 `json:"attemptSeq"`
	Slot         int    `json:"slot"`
	Stage        string `json:"stage"`
	Reason       string `json:"reason"`
	Received     bool   `json:"received"`
	SampleMillis int64  `json:"sampleMillis,omitempty"`
}

func (d InputDiagnostic) valid() bool {
	runtime, err := tokenBytes(d.Runtime)
	if err != nil || runtime == [16]byte{} || d.Schema != InputDiagnosticSchema || d.AttemptSeq == 0 || d.AttemptSeq > 9007199254740991 || d.Slot < 1 || d.Slot > bridge.SlotCount+1 || d.SampleMillis < 0 {
		return false
	}
	switch d.Stage {
	case "entered", "loading":
		return d.Reason == ""
	case "received":
		return d.Received && d.Reason == ""
	case "load_failed":
		return d.Reason == "slot_load_failed"
	case "no_dispatch":
		return d.Reason == "slot_loader_no_dispatch"
	case "exception":
		return d.Reason == "slot_wake_failed"
	case "rejected":
		switch d.Reason {
		case "slot_input_not_ready", "input_protection_not_ready", "input_protection_busy", "input_protection_unavailable", "input_protection_timer_unavailable", "slot_exhausted", "slot_inventory_incomplete", "slot_wake_rejected":
			return true
		}
	}
	return false
}

func parseInputDiagnostic(raw json.RawMessage, runtime string, sample int64) *InputDiagnostic {
	if len(raw) == 0 || len(raw) > 512 {
		return nil
	}
	var d InputDiagnostic
	wire := struct {
		*InputDiagnostic
		Received *bool   `json:"received"`
		Reason   *string `json:"reason"`
	}{InputDiagnostic: &d}
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	if decoder.Decode(&wire) != nil || decoder.Decode(new(any)) != io.EOF || wire.Received == nil || wire.Reason == nil {
		return nil
	}
	d.Received, d.Reason = *wire.Received, *wire.Reason
	if !d.valid() || d.Runtime != runtime {
		return nil
	}
	d.SampleMillis = sample
	return &d
}

// Bad optional diagnostic data cannot hide a real readiness/focus/target error.
func (s *InputObservation) UnmarshalJSON(data []byte) error {
	type plain InputObservation
	*s = InputObservation{}
	wire := struct {
		*plain
		Diagnostic json.RawMessage `json:"inputAttempt"`
	}{plain: (*plain)(s)}
	if err := json.Unmarshal(data, &wire); err != nil {
		return err
	}
	s.InputDiagnostic = parseInputDiagnostic(wire.Diagnostic, s.Runtime, s.SampleMillis)
	return nil
}

type receiptAttemptProbe struct{ exchange string }

// At most one named INPUT read per exchange in this native invocation. It shares
// the normal physical/read/trace budgets and never adds a timer or retry.
func (p *receiptAttemptProbe) observe(ctx context.Context, reader RecordReader, q ObservationQuery, observation Observation, original error, now int64) (Observation, error) {
	if err := ctx.Err(); err != nil {
		return observation, errors.Join(original, err)
	}
	if reader == nil || q.Kind != "receipt" || !errors.Is(original, ErrPending) || q.Envelope.Nonce == p.exchange {
		return observation, original
	}
	p.exchange = q.Envelope.Nonce
	runtime, err := tokenBytes(q.Envelope.Runtime)
	if err != nil {
		return observation, original
	}
	found, err := reader.Find(ctx, memory.Selector{Kind: bridge.MemoryInputState, Runtime: runtime, Nonce: runtime}, true)
	if ctx.Err() != nil {
		return observation, errors.Join(original, err, ctx.Err())
	}
	if err != nil {
		return observation, errors.Join(original, err)
	}
	if len(found.Records) != 1 {
		return observation, original
	}
	record := found.Records[0]
	if len(record.Payload) > 2048 {
		return observation, original
	}
	var current InputObservation
	if json.Unmarshal(record.Payload, &current) != nil || current.NextSlot < 1 || current.NextSlot > bridge.SlotCount+1 {
		return observation, original
	}
	// The counter is explanatory even when Receive already advanced the slot.
	// Reuse static identity/freshness checks without granting input for that slot.
	target := q.Envelope
	target.Index, target.StartSlot = current.NextSlot, 0
	current, err = inputObservation(record, target, 0, now)
	if ctx.Err() != nil {
		return observation, errors.Join(original, ctx.Err())
	}
	if err != nil {
		return observation, original
	}
	observation.InputDiagnostic = current.InputDiagnostic
	return observation, original
}

func (d *Driver) recordInputDiagnostic(ctx context.Context, tx *Transaction, diagnostic *InputDiagnostic) error {
	a := d.State.Input
	if diagnostic == nil || !diagnostic.valid() || a == nil || a.Observation == nil || a.Kind != "invoke" || a.Exchange != tx.Envelope.Nonce || a.Runtime != diagnostic.Runtime || a.Runtime != tx.Envelope.Runtime || diagnostic.Slot < slotStart(tx.Envelope) || diagnostic.Slot > tx.Envelope.Index || diagnostic.SampleMillis < a.Observation.SampleMillis {
		return nil
	}
	var before, after uint64
	if a.Observation.InputDiagnostic != nil {
		before = a.Observation.InputDiagnostic.AttemptSeq
	}
	if a.AfterDiagnostic != nil {
		after = a.AfterDiagnostic.AttemptSeq
	}
	if diagnostic.AttemptSeq <= before || diagnostic.AttemptSeq <= after {
		return nil
	}
	copy := *diagnostic
	a.AfterDiagnostic = &copy
	return d.Save(ctx, "input_attempt_diagnostic")
}
