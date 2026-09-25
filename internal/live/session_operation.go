package live

import (
	"context"
	"encoding/json"
	"errors"
	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"os"
	"time"
)

type signalWait func(context.Context, bridge.SignalExpectation) (bridge.Signal, error)

func (s *WindowSession) operationInput(ctx context.Context, root string, record journal.WorkRecord) (ReportIntent, error) {
	input, binding, err := s.boundOperationInput(ctx, root, record)
	if err != nil {
		return ReportIntent{}, err
	}
	ready, err := operationAnchor(ctx, root, record, input, binding)
	if err != nil {
		return ReportIntent{}, err
	}
	if ready.RuntimeEpoch != s.ready.RuntimeEpoch {
		return ReportIntent{}, errors.New("live.operation_runtime_mismatch")
	}
	return input, nil
}

// Historical runtime anchor, not current input permission. Live senders must
// still obtain fresh readiness from their selected capture stream.
func operationAnchor(ctx context.Context, root string, record journal.WorkRecord, input ReportIntent, binding RecordedSession) (bridge.Signal, error) {
	base, err := executionBase(ctx, root, record, input, binding)
	if err != nil {
		return bridge.Signal{}, err
	}
	ready := base.Ready
	var observed struct {
		ReloadedCapture string `json:"reloadedCapture"`
		AckReadyCapture string `json:"ackReadyCapture"`
		ClearedID       string `json:"clearedId"`
		CleanupReadyID  string `json:"cleanupReadyId"`
	}
	if len(record.Observation) != 0 {
		if err := json.Unmarshal(record.Observation, &observed); err != nil {
			return bridge.Signal{}, err
		}
	}
	if observed.ReloadedCapture != "" {
		ready, _, err = readReloadEvidence(ctx, root, record, input, binding, observed.ReloadedCapture)
		if err != nil {
			return bridge.Signal{}, err
		}
	}
	if observed.AckReadyCapture != "" {
		ready, err = readAckReadiness(ctx, root, record, input, binding, ready, observed.AckReadyCapture, observed.ReloadedCapture != "")
		if err != nil {
			return bridge.Signal{}, err
		}
	}
	if observed.CleanupReadyID != "" && observed.ClearedID != "" {
		var cleanup reportObservation
		if err := json.Unmarshal(record.Observation, &cleanup); err != nil {
			return bridge.Signal{}, err
		}
		ready, _, err = readCleanupEvidence(ctx, root, record, input, binding, cleanup)
		if err != nil {
			return bridge.Signal{}, err
		}
	}
	return ready, nil
}

// Shared immutable window/binding checks. Only explicit reload reconciliation
// may call this without the current-runtime check performed by operationInput.
func (s *WindowSession) boundOperationInput(ctx context.Context, root string, record journal.WorkRecord) (ReportIntent, RecordedSession, error) {
	var zero ReportIntent
	if s == nil || s.closed || s.reader == nil || s.confirm == nil {
		return zero, RecordedSession{}, errors.New("live.session_closed")
	}
	input, _, err := probeDefinition(record)
	if err != nil {
		return zero, RecordedSession{}, err
	}
	if !s.matchesIdentity(input.Expected) || input.Load.GUID != s.ready.GUID {
		return zero, RecordedSession{}, errors.New("live.operation_session_mismatch")
	}
	if input.Binding == "" {
		return zero, RecordedSession{}, errors.New("live.operation_binding_missing")
	}
	binding, err := ReadWindowSession(ctx, root, input.Binding)
	if err != nil {
		return zero, RecordedSession{}, err
	}
	if binding.Record.Snapshot != record.Intent.Snapshot || binding.Target != s.target || binding.Ready.SessionNonce != s.ready.SessionNonce || binding.Ready.GUID != s.ready.GUID {
		return zero, RecordedSession{}, errors.New("live.operation_binding_mismatch")
	}
	if err := s.matchInstallation(input.Load.Installation); err != nil {
		return zero, RecordedSession{}, err
	}
	return input, binding, nil
}

// The operation handle supplies the guard; a nil guard is only used by focused
// receipt tests, never by the public session entry point.
func (s *WindowSession) observeOperation(ctx context.Context, root, operationID string, guard func(context.Context) error) (evidence.CaptureRef, error) {
	var zero evidence.CaptureRef
	record, err := InspectOperation(ctx, root, operationID)
	if err != nil {
		return zero, err
	}
	input, err := s.operationInput(ctx, root, record)
	if err != nil {
		return zero, err
	}
	wait, cancel := context.WithTimeout(ctx, 15*time.Second)
	defer cancel()
	observe := func(ctx context.Context, expected bridge.SignalExpectation) (bridge.Signal, error) {
		if guard != nil {
			if err := guard(ctx); err != nil {
				return bridge.Signal{}, err
			}
		}
		signal, err := s.waitOperationSignal(ctx, input.Load.Installation, expected)
		if err == nil && guard != nil {
			err = guard(ctx)
		}
		return signal, err
	}
	switch record.Stage {
	case "load_requested", "loaded":
		return observeOperationLoaded(wait, root, operationID, observe)
	case "dispatch_requested":
		return observeOperationReported(wait, root, operationID, observe)
	case "ack_requested":
		return observeReportAcknowledgement(wait, root, operationID, observe)
	default:
		return zero, journal.ErrTransition
	}
}

func (s *WindowSession) matchesIdentity(expected bridge.SignalExpectation) bool {
	ready := s.ready
	return expected.Release == ready.Release && expected.SessionNonce == ready.SessionNonce &&
		expected.Character == ready.Character && expected.Realm == ready.Realm &&
		expected.Product == ready.Product && expected.Build == ready.Build
}

func (s *WindowSession) matchInstallation(installation string) error {
	selected, err := os.Stat(s.target.Client.Directory)
	if err != nil {
		return err
	}
	requested, err := os.Stat(installation)
	if err != nil {
		return err
	}
	if !selected.IsDir() || !requested.IsDir() || !os.SameFile(selected, requested) {
		return errors.New("live.operation_installation_mismatch")
	}
	return nil
}

func (s *WindowSession) waitOperationSignal(ctx context.Context, installation string, expected bridge.SignalExpectation) (bridge.Signal, error) {
	var zero bridge.Signal
	if !s.matchesIdentity(expected) {
		return zero, errors.New("live.operation_session_mismatch")
	}
	if err := s.confirm(ctx, s.target); err != nil {
		return zero, err
	}
	if err := s.matchInstallation(installation); err != nil {
		return zero, err
	}
	if expected.AfterSequence < s.ready.Sequence {
		expected.AfterSequence = s.ready.Sequence
	}
	if expected.Kind == "ready" {
		// Ordinary input readiness cannot silently cross a Lua runtime change.
		// A correlated reload needs its own explicit transition, not this path.
		expected.RuntimeEpoch = s.ready.RuntimeEpoch
	}
	signal, err := s.reader.WaitForSignal(ctx, expected)
	if err != nil {
		return zero, err
	}
	// Revalidate before the phase implementation archives or advances work.
	if err := s.confirm(ctx, s.target); err != nil {
		return zero, err
	}
	if err := s.matchInstallation(installation); err != nil {
		return zero, err
	}
	return signal, nil
}
