package channel

import (
	"context"
	"encoding/json"
	"errors"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/live/memory"
)

var errInputStateStale = errors.New("live.channel_input_state_stale")

// A bounded wait for a structurally valid, matching but out-of-date sample.
// This scheduling hint grants no input authority and proves no live runtime.
var ErrInputObservationStale = errors.Join(ErrPending, errors.New("live.channel_input_observation_stale"))

type InputObservation struct {
	Schema       string               `json:"schema"`
	Runtime      string               `json:"runtime"`
	Owner        string               `json:"owner"`
	Fence        uint64               `json:"fence"`
	NextSlot     int                  `json:"nextSlot"`
	GUID         string               `json:"guid"`
	Build        string               `json:"build"`
	SampleMillis int64                `json:"sampleMillis"`
	InputBlocked *bool                `json:"inputBlocked"`
	Reason       string               `json:"reason"`
	Address      uint64               `json:"-"`
	Optical      *InputSignalEvidence `json:"optical,omitempty"`
}

type InputAction struct {
	Kind        string              `json:"kind"`
	Envelope    bridge.SlotEnvelope `json:"-"`
	Observation *InputObservation   `json:"observation,omitempty"`
	Capability  string              `json:"capability,omitempty"`
}
type InputOutcome struct {
	Disposition    string `json:"disposition"` // not_sent, submitted, uncertain
	MessagesQueued int    `json:"messagesQueued"`
	AtMillis       int64  `json:"atMillis"`
	Reason         string `json:"reason,omitempty"`
	Retryable      bool   `json:"retryable,omitempty"`
}
type InputAttempt struct {
	ID          string            `json:"id"`
	Exchange    string            `json:"exchange"`
	Runtime     string            `json:"runtime"`
	Kind        string            `json:"kind"`
	Observation *InputObservation `json:"observation,omitempty"`
	Outcome     *InputOutcome     `json:"outcome,omitempty"`
}

// perform is the sole journal boundary for connected physical input. An intent
// without an outcome is uncertain after restart; it is never inferred unsent.
func (d *Driver) perform(ctx context.Context, exchange, runtime string, action InputAction) (InputOutcome, error) {
	id, err := token()
	if err != nil {
		return InputOutcome{}, err
	}
	d.State.Input = &InputAttempt{ID: id, Exchange: exchange, Runtime: runtime, Kind: action.Kind, Observation: action.Observation}
	if err = d.Save(ctx, "input_intent"); err != nil {
		return InputOutcome{}, err
	}
	out, sendErr := d.Backend.Input(ctx, action)
	if out.validate() != nil {
		out = InputOutcome{Disposition: "uncertain", Reason: "invalid_transport_outcome"}
		sendErr = errors.New("live.channel_input_outcome_invalid")
	}
	d.State.Input.Outcome = &out
	// Cancellation ends input, not the bounded persistence of known facts.
	flush, cancel := context.WithTimeout(context.WithoutCancel(ctx), 3*time.Second)
	defer cancel()
	if err = d.Save(flush, "input_observed"); err != nil {
		return out, err
	}
	return out, sendErr
}
func (o InputOutcome) validate() error {
	if o.MessagesQueued < 0 || o.AtMillis < 0 || (o.Disposition != "not_sent" && o.Disposition != "submitted" && o.Disposition != "uncertain") || o.Disposition == "not_sent" && o.MessagesQueued != 0 || o.Disposition == "submitted" && o.MessagesQueued == 0 || o.Retryable && o.Disposition != "not_sent" {
		return errors.New("live.channel_input_outcome_invalid")
	}
	return nil
}
func (d *Driver) invoke(ctx context.Context, tx *Transaction) error {
	if err := d.submitInput(ctx, tx.Envelope.Nonce, tx.Envelope.Runtime, "invoke", tx.Envelope); err != nil {
		return err
	}
	tx.Phase = "input_attempted"
	return d.Save(ctx, "input_attempted")
}

// submitInput shares readiness and single-Escape decisions between slot input
// and observed reload. Only bootstrap without telemetry uses fixed fallback.
func (d *Driver) submitInput(ctx context.Context, exchange, runtime, desired string, envelope bridge.SlotEnvelope) error {
	if d.State.RuntimeEnd != nil {
		return runtimeRetirementPending()
	}
	var after int64
	if previous := d.State.Input; previous != nil && previous.Exchange == exchange && previous.Runtime == runtime {
		if previous.Kind == desired && (previous.Outcome == nil || previous.Outcome.Disposition != "not_sent") {
			return nil
		}
		if previous.Outcome != nil {
			after = previous.Outcome.AtMillis
		} else if previous.Observation != nil {
			// Lost Escape outcomes may only advance from a newer observation,
			// never from the same pre-input sample restored from the journal.
			after = previous.Observation.SampleMillis
		}
	}
	s, err := d.Backend.ObserveInput(ctx, envelope, after, d.State.Identity.InputState)
	if err != nil {
		d.Waiting = "input_observation_unavailable"
		var signalErr *inputSignalPending
		if errors.As(err, &signalErr) {
			d.Waiting = signalErr.reason
		}
		if errors.Is(err, ErrInputObservationStale) {
			d.Waiting = "input_observation_stale"
		}
		return err
	}
	if s.InputBlocked == nil {
		d.Waiting = "input_observation_unavailable"
		return ErrPending
	}
	kind := desired
	if *s.InputBlocked {
		d.Waiting = s.Reason
		if s.Reason != "input_keyboard_focus" {
			return ErrPending
		}
		kind = "escape"
	}
	out, err := d.perform(ctx, exchange, runtime, InputAction{Kind: kind, Envelope: envelope, Observation: &s, Capability: d.State.Identity.InputState})
	if out.Disposition == "not_sent" {
		d.Waiting = out.Reason
		if out.Retryable {
			return errors.Join(ErrPending, err)
		}
		if err == nil {
			return errors.New("live.channel_input_not_sent")
		}
		return err
	}
	if err != nil {
		return err
	}
	if kind == "escape" {
		d.Waiting = "input_keyboard_focus"
		return ErrPending
	}
	return nil
}

// Same-machine uptime is a freshness gate, not a replacement for nonce receipt
// verification. A missing/stale sample cannot authorize any key, including Esc.
func inputObservation(r memory.Record, e bridge.SlotEnvelope, after, now int64) (InputObservation, error) {
	var s InputObservation
	if r.Header.Kind != bridge.MemoryInputState || json.Unmarshal(r.Payload, &s) != nil || s.Schema != "lycheedev.input.v1" || s.InputBlocked == nil {
		return s, errors.New("live.channel_input_state_invalid")
	}
	// Reload consumes no slot. A lost receipt must not disable the control
	// action merely because the game has already advanced its slot counter.
	if s.Runtime != e.Runtime || s.GUID != e.GUID || s.Build != e.Build || (e.Action != "reload" && s.NextSlot != slotStart(e)) || (s.Owner != e.Owner && !(e.Action == "bind" && s.Owner == "")) || (s.Owner != "" && s.Fence != e.Fence) {
		return s, errors.New("live.channel_input_target_changed")
	}
	if !*s.InputBlocked && s.Reason != "" || *s.InputBlocked && s.Reason == "" {
		return s, errors.New("live.channel_input_state_invalid")
	}
	if s.SampleMillis < 0 || s.SampleMillis > now+100 {
		return s, errors.New("live.channel_input_clock_invalid")
	}
	if s.SampleMillis < after+100 || s.SampleMillis < now-500 {
		return s, errInputStateStale
	}
	return s, nil
}

// records must be the successful lookup's already-Accepted records. Preserve
// target/format checks; only post-lookup freshness expiry avoids eager discovery.
func inputAfterLookup(records []memory.Record, e bridge.SlotEnvelope, after, now int64) (InputObservation, error) {
	if len(records) == 0 {
		return InputObservation{}, ErrPending
	}
	s, err := inputObservation(records[0], e, after, now)
	if errors.Is(err, errInputStateStale) {
		return InputObservation{}, ErrInputObservationStale
	}
	if err != nil {
		return InputObservation{}, ErrPending
	}
	s.Address = records[0].Address
	return s, nil
}

func slotStart(e bridge.SlotEnvelope) int {
	if e.StartSlot > 0 {
		return e.StartSlot
	}
	return e.Index
}
