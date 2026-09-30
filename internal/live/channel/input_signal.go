package channel

import (
	"errors"

	"github.com/follenfang/lycheedev/internal/bridge"
)

// Optical evidence describes rendered colors, never a Lua sample timestamp or
// runtime identity. Identity and sample freshness remain independent gates.
type InputSignalEvidence struct {
	State      string `json:"state"`
	Heartbeat  bool   `json:"heartbeat"`
	FrameTicks int64  `json:"frameTicks"`
	EdgeTicks  int64  `json:"edgeTicks"`
}

const signalTicksPerMillisecond int64 = 10000
const signalFrameMaxAge = 500 * signalTicksPerMillisecond
const signalEdgeMaxAge = 1500 * signalTicksPerMillisecond

type inputSignalPending struct{ reason string }

func (e *inputSignalPending) Error() string { return "live.channel_" + e.reason }
func (e *inputSignalPending) Unwrap() error { return ErrPending }

func observedInputCapability(capability string) bool {
	return capability == "lycheedev.input.v1" || capability == bridge.InputSignalCapability
}

type inputSignalTracker struct {
	current   InputSignalEvidence
	have      bool
	after     int64
	highwater int64
}

func (t *inputSignalTracker) invalidate() { t.current = InputSignalEvidence{}; t.have = false }

func (t *inputSignalTracker) rejectFrame(ticks, now int64) {
	if ticks > t.highwater && ticks <= now {
		t.highwater = ticks
	}
	t.invalidate()
}

func (t *inputSignalTracker) accept(signal bridge.InputSignal, ticks, now int64) {
	if ticks <= 0 || ticks > now || now-ticks > signalFrameMaxAge || ticks <= t.highwater {
		t.invalidate()
		return
	}
	t.highwater = ticks
	edge := t.current.EdgeTicks
	if !t.have || ticks-t.current.FrameTicks > signalEdgeMaxAge {
		edge = 0
	} else if signal.Heartbeat != t.current.Heartbeat {
		edge = ticks
	}
	t.current = InputSignalEvidence{State: signal.State, Heartbeat: signal.Heartbeat, FrameTicks: ticks, EdgeTicks: edge}
	t.have = true
}

func (t *inputSignalTracker) evidence(now int64) (InputSignalEvidence, error) {
	s := t.current
	if !t.have || now < s.FrameTicks {
		t.invalidate()
		return InputSignalEvidence{}, &inputSignalPending{"input_signal_unavailable"}
	}
	if now-s.FrameTicks > signalFrameMaxAge {
		// An observed expiry denies this frame and its previous edge. Retain
		// only bounded decoded heartbeat comparison history so a subsequent
		// fresh flip can establish a new edge. Actual invalid frames and stream
		// resets still invalidate; a same-heartbeat frame cannot revive input.
		if now-s.FrameTicks > signalEdgeMaxAge {
			t.invalidate()
		} else {
			t.current.EdgeTicks = 0
		}
		return InputSignalEvidence{}, &inputSignalPending{"input_signal_unavailable"}
	}
	if s.EdgeTicks == 0 || now < s.EdgeTicks || now-s.EdgeTicks > signalEdgeMaxAge || s.EdgeTicks < t.after {
		return InputSignalEvidence{}, &inputSignalPending{"input_signal_waiting"}
	}
	switch s.State {
	case "ready", "input_keyboard_focus":
		return s, nil
	case "input_combat_lockdown":
		return InputSignalEvidence{}, &inputSignalPending{s.State}
	default:
		return InputSignalEvidence{}, &inputSignalPending{"input_signal_unknown"}
	}
}

func (t *inputSignalTracker) afterInput(ticks int64) {
	if ticks <= 0 {
		t.invalidate()
		return
	}
	t.after = ticks + 100*signalTicksPerMillisecond
}

// Agreement is deliberately conservative during migration: an old memory
// sample cannot override a newly blocked optical state, or vice versa.
func inputSignalAgrees(signal InputSignalEvidence, memory InputObservation) bool {
	if memory.InputBlocked == nil {
		return false
	}
	if !*memory.InputBlocked {
		return signal.State == "ready" && memory.Reason == ""
	}
	return memory.Reason == "input_keyboard_focus" && signal.State == memory.Reason
}

func inputCapabilityError(capability string) error {
	if capability == "" || observedInputCapability(capability) {
		return nil
	}
	return errors.New("live.channel_input_capability_unsupported")
}

func observeHybridInput(signal func() (InputSignalEvidence, error), lookup func() (InputObservation, error)) (InputObservation, error) {
	return observeHybridInputWithObserver(signal, lookup, nil)
}

func observeHybridInputWithObserver(signal func() (InputSignalEvidence, error), lookup func() (InputObservation, error), observer func(post bool, err error)) (InputObservation, error) {
	record := func(post bool, err error) {
		if observer != nil {
			observer(post, err)
		}
	}
	if _, err := signal(); err != nil {
		record(false, err)
		return InputObservation{}, err
	}
	record(false, nil)
	s, err := lookup()
	if err != nil {
		return InputObservation{}, err
	}
	// Discovery can take seconds. Never attach a pre-scan green frame to the
	// newly returned memory evidence.
	optical, err := signal()
	if err != nil {
		record(true, err)
		return InputObservation{}, err
	}
	if !inputSignalAgrees(optical, s) {
		err := &inputSignalPending{"input_signal_changed"}
		record(true, err)
		return InputObservation{}, err
	}
	record(true, nil)
	s.Optical = &optical
	return s, nil
}
