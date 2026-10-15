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

const maxInputEscapes = 3
const legacyInputSchema = "lycheedev.input.v3"

// A bounded wait for a structurally valid, matching but out-of-date sample.
// This scheduling hint grants no input authority and proves no live runtime.
var ErrInputObservationStale = errors.Join(ErrPending, errors.New("live.channel_input_observation_stale"))

type InputObservation struct {
	Bindings           *InputBindings   `json:"bindings"`
	Schema             string           `json:"schema"`
	Runtime            string           `json:"runtime"`
	Owner              string           `json:"owner"`
	Fence              uint64           `json:"fence"`
	NextSlot           int              `json:"nextSlot"`
	GUID               string           `json:"guid"`
	Build              string           `json:"build"`
	SampleMillis       int64            `json:"sampleMillis"`
	InputBlocked       *bool            `json:"inputBlocked"`
	SlotReloadRequired bool             `json:"slotReloadRequired,omitempty"`
	Reason             string           `json:"reason"`
	EscapeLimit        int              `json:"escapeLimit"`
	Address            uint64           `json:"-"`
	InputDiagnostic    *InputDiagnostic `json:"inputAttempt,omitempty"`
	// Host uptime of this verified current INPUT read, never supplied by the
	// addon or restored from an old journal's pre-input observation.
	ObservedAtMillis int64 `json:"-"`
}

type InputAction struct {
	Kind        string              `json:"kind"`
	Envelope    bridge.SlotEnvelope `json:"-"`
	Observation *InputObservation   `json:"observation,omitempty"`
	Capability  string              `json:"capability,omitempty"`
	AfterMillis int64               `json:"afterMillis,omitempty"`
}
type InputOutcome struct {
	Disposition    string `json:"disposition"` // not_sent, submitted, uncertain
	MessagesQueued int    `json:"messagesQueued"`
	AtMillis       int64  `json:"atMillis"`
	Reason         string `json:"reason,omitempty"`
	Retryable      bool   `json:"retryable,omitempty"`
}
type InputAttempt struct {
	ID       string `json:"id"`
	Exchange string `json:"exchange"`
	Runtime  string `json:"runtime"`
	Kind     string `json:"kind"`
	// Preserve the prior runtime input watermark even if this attempt proves
	// not_sent before the native transport can provide a timestamp.
	AfterMillis int64 `json:"afterMillis,omitempty"`
	// A post-input observation bounds an unknown submission time after crash
	// recovery. Persist it once; polling must not move the cadence window.
	RecoveryAfterMillis int64             `json:"recoveryAfterMillis,omitempty"`
	EscapeAttempts      int               `json:"escapeAttempts,omitempty"`
	Observation         *InputObservation `json:"observation,omitempty"`
	Outcome             *InputOutcome     `json:"outcome,omitempty"`
	AfterDiagnostic     *InputDiagnostic  `json:"afterDiagnostic,omitempty"`
}

// perform is the sole journal boundary for connected physical input. An intent
// without an outcome is uncertain after restart; it is never inferred unsent.
func (d *Driver) perform(ctx context.Context, exchange, runtime string, action InputAction) (InputOutcome, error) {
	previous := d.State.Input
	if previous != nil && previous.Exchange == exchange && previous.Runtime == runtime && previous.Kind == action.Kind && previous.Outcome != nil && previous.Outcome.Disposition == "not_sent" && previous.Outcome.MessagesQueued == 0 {
		// Repeated native key blockers can outlive one log segment. Preserve the
		// complete known-zero history before recording another retry intent.
		if err := d.checkpointLog(ctx); err != nil {
			return InputOutcome{}, err
		}
	}
	id, err := token()
	if err != nil {
		return InputOutcome{}, err
	}
	d.State.Input = &InputAttempt{ID: id, Exchange: exchange, Runtime: runtime, Kind: action.Kind, Observation: action.Observation, AfterMillis: action.AfterMillis}
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

// submitInput shares readiness between slot input and direct reload. Focus is
// handled by the addon; the host never sends Escape or chat text.
func (d *Driver) submitInput(ctx context.Context, exchange, runtime, desired string, envelope bridge.SlotEnvelope) error {
	if d.State.RuntimeEnd != nil {
		return runtimeRetirementPending()
	}
	var after int64
	recoverTime := false
	previous := d.State.Input
	if previous != nil && previous.Runtime == runtime {
		if previous.Exchange == exchange && previous.Kind == desired && (previous.Outcome == nil || previous.Outcome.Disposition != "not_sent") {
			return nil
		}
		// Cadence belongs to the runtime, not a nonce. finishTransaction keeps
		// Input, so adjacent prepare/commit/confirm exchanges and resumed logs
		// must all wait for a sample taken at least 100ms after the last input.
		after = max(previous.AfterMillis, previous.RecoveryAfterMillis)
		if previous.Outcome != nil {
			after = max(after, previous.Outcome.AtMillis)
		}
		// A saved pre-input sample or a previous exchange's watermark cannot
		// bound when this attempt sent input. A missing/zero-time outcome must
		// first acquire a current host-uptime bound, without inventing an outcome.
		recoverTime = previous.RecoveryAfterMillis == 0 && (previous.Outcome == nil || previous.Outcome.Disposition != "not_sent" && previous.Outcome.AtMillis == 0)
	}
	s, err := d.observeInput(ctx, envelope, after)
	if err != nil {
		return err
	}
	if *s.InputBlocked {
		d.Waiting = s.Reason
		return ErrPending
	}
	if recoverTime {
		if s.ObservedAtMillis <= 0 {
			d.Waiting = "input_observation_unavailable"
			return ErrPending
		}
		// Normal continuation has already observed the original receipt. This
		// later verified INPUT read is therefore a conservative upper bound on
		// that input, including journals written before this field existed.
		previous.RecoveryAfterMillis = s.ObservedAtMillis
		after = max(after, previous.RecoveryAfterMillis)
		if err := d.Save(ctx, "input_time_recovered"); err != nil {
			return err
		}
		if s.SampleMillis < after+100 {
			d.Waiting = "input_observation_stale"
			return ErrInputObservationStale
		}
	}
	d.Waiting = ""
	if desired == "invoke" && s.SlotReloadRequired {
		return d.requestSlotReload(ctx)
	}
	out, err := d.perform(ctx, exchange, runtime, InputAction{Kind: desired, Envelope: envelope, Observation: &s, Capability: d.State.Identity.InputState, AfterMillis: after})
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
	return nil
}

func (d *Driver) observeInput(ctx context.Context, envelope bridge.SlotEnvelope, after int64) (InputObservation, error) {
	s, err := d.Backend.ObserveInput(ctx, envelope, after, d.State.Identity.InputState)
	if err != nil {
		d.Waiting = "input_observation_unavailable"
		if errors.Is(err, ErrInputObservationStale) {
			d.Waiting = "input_observation_stale"
		}
		return s, err
	}
	if !s.validReadiness() || !s.validBindings() {
		d.Waiting = "input_observation_unavailable"
		return s, ErrPending
	}
	return s, nil
}

// Same-machine uptime is a freshness gate, not a replacement for nonce receipt
// verification. A missing/stale sample cannot authorize any key, including Esc.
func inputObservation(r memory.Record, e bridge.SlotEnvelope, after, now int64) (InputObservation, error) {
	var s InputObservation
	h := r.Header
	if len(r.Payload) > 2048 || h.Kind != bridge.MemoryInputState || h.State != 1 || h.Sequence == 0 || h.Sequence == ^uint32(0) || h.Ticket != [16]byte{} || json.Unmarshal(r.Payload, &s) != nil || !s.validSchema(e.Action) || !s.validReadiness() || !s.validBindings() {
		return s, errors.New("live.channel_input_state_invalid")
	}
	runtime, err := tokenBytes(s.Runtime)
	if err != nil || runtime == [16]byte{} || h.Runtime != runtime || h.Nonce != runtime {
		return s, errors.New("live.channel_input_state_invalid")
	}
	// Reload consumes no slot. A lost receipt must not disable the control
	// action merely because the game has already advanced its slot counter.
	if s.Runtime != e.Runtime || s.GUID != e.GUID || s.Build != e.Build || (e.Action != "reload" && s.NextSlot != slotStart(e)) || (s.Owner != e.Owner && !(e.Action == "bind" && s.Owner == "")) || (s.Owner != "" && s.Fence != e.Fence) {
		return s, errors.New("live.channel_input_target_changed")
	}
	if s.SampleMillis < 0 || s.SampleMillis > now+100 {
		return s, errors.New("live.channel_input_clock_invalid")
	}
	if s.SampleMillis < after+100 || s.SampleMillis < now-500 {
		return s, errInputStateStale
	}
	s.ObservedAtMillis = now
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

func observedInputCapability(capability string) bool { return capability == InputSchema }
func reloadInputCapability(capability string) bool {
	return observedInputCapability(capability) || capability == bridge.PreviousMailboxSchema
}

// The previous schema name covered both the retired three-key receiver and
// the later two-key runtime. Inspect its actual INPUT before selecting automatic
// activation. A stale valid profile proves capability only; submitInput still
// requires a fresh sample before sending any key.
func activationInputCapability(ctx context.Context, reader RecordReader, identity Identity, now int64) (bool, error) {
	if identity.InputState != bridge.PreviousMailboxSchema {
		return reloadInputCapability(identity.InputState), nil
	}
	runtime, _ := tokenBytes(identity.Runtime)
	found, err := reader.Find(ctx, memory.Selector{Runtime: runtime, Nonce: runtime, Kind: bridge.MemoryInputState}, true)
	if err != nil {
		return false, err
	}
	if len(found.Records) != 1 {
		return false, nil
	}
	e := bridge.SlotEnvelope{Runtime: identity.Runtime, Owner: identity.Owner, Fence: identity.Fence, Index: identity.NextSlot, GUID: identity.GUID, Build: identity.Build, Action: "reload"}
	s, err := inputObservation(found.Records[0], e, 0, now)
	return (err == nil || errors.Is(err, errInputStateStale)) && s.Bindings != nil && s.Bindings.Reload != "", nil
}

func upgradeInputCapability(capability string) bool {
	return capability == legacyInputSchema || capability == "lycheedev.input.hybrid.v2" || capability == bridge.PreviousMailboxSchema
}

func inputCapabilityError(capability string) error {
	if observedInputCapability(capability) {
		return nil
	}
	return errors.New("live.channel_input_capability_unsupported")
}

func inputActionCapabilityError(capability, action string) error {
	if action == "reload" && reloadInputCapability(capability) {
		return nil
	}
	return inputCapabilityError(capability)
}

func (s InputObservation) validSchema(action string) bool {
	return s.Schema == InputSchema || (action == "reload" && s.Schema == bridge.PreviousMailboxSchema)
}

func (s InputObservation) validReadiness() bool {
	return s.InputBlocked != nil && (*s.InputBlocked == (s.Reason != "")) && s.EscapeLimit == 0
}
