//go:build windows && amd64

package channel

import (
	"context"
	"errors"
	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/delivery"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/live/memory"
	"golang.org/x/sys/windows"
	"sync/atomic"
	"time"
)

var inputUptime = windows.NewLazySystemDLL("kernel32.dll").NewProc("GetTickCount64")

func uptimeMillis() int64 { n, _, _ := inputUptime.Call(); return int64(n) }

func (n *Native) ObserveInput(ctx context.Context, e bridge.SlotEnvelope, after int64, capability string) (InputObservation, error) {
	if err := inputCapabilityError(capability); err != nil {
		return InputObservation{}, err
	}
	if n.Guard == nil {
		return InputObservation{}, errors.New("live.channel_guard_required")
	}
	if err := n.Guard(ctx); err != nil {
		return InputObservation{}, err
	}
	lookup := func() (InputObservation, error) {
		return n.observeInputFrom(ctx, n.Process, e, after, uptimeMillis, time.Now)
	}
	if capability != bridge.InputSignalCapability {
		return lookup()
	}
	return observeHybridInput(func() (InputSignalEvidence, error) { return n.observeSignal(ctx, e.Runtime) }, lookup)
}

func (n *Native) observeInputFrom(ctx context.Context, source memory.Source, e bridge.SlotEnvelope, after int64, uptime func() int64, clock func() time.Time) (InputObservation, error) {
	n.observationRuntime(e.Runtime)
	ctx, cancel := n.observationContext(ctx)
	defer cancel()
	if err := ctx.Err(); err != nil {
		return InputObservation{}, err
	}
	if err := n.memorySession().Err(); err != nil {
		return InputObservation{}, err
	}
	key := inputSampleKey(e, after)
	// Cache-off has no retained address to observe cheaply. A completed scan
	// that found only recent stale samples may defer another discovery within
	// one fixed grace interval; the next observation still scans the process.
	if n.Hints == nil && n.inputWait.waiting(key, clock()) {
		return InputObservation{}, ErrInputObservationStale
	}
	runtime, _ := tokenBytes(e.Runtime)
	selector := memory.Selector{Runtime: runtime, Nonce: runtime, Kind: bridge.MemoryInputState}
	var stale atomic.Bool
	var recentStaleMillis atomic.Int64
	recentStaleMillis.Store(-1)
	accept := func(r memory.Record, recent bool) bool {
		now := uptime()
		s, err := inputObservation(r, e, after, now)
		if errors.Is(err, errInputStateStale) {
			stale.Store(true)
			if s.SampleMillis >= now-inputRecentHintMillis {
				for previous := recentStaleMillis.Load(); s.SampleMillis > previous; previous = recentStaleMillis.Load() {
					if recentStaleMillis.CompareAndSwap(previous, s.SampleMillis) {
						break
					}
				}
			}
			// Cached discovery may stop at a recent sample for cheap local
			// cadence observation. Cache-off must continue past stale prefixes
			// so an already published fresh sample is not hidden behind them.
			return recent && n.Hints != nil && s.SampleMillis >= now-inputRecentHintMillis && (n.inputHintRuntime != e.Runtime || s.SampleMillis > n.inputHintMillis)
		}
		return err == nil
	}
	selector.Accept = func(r memory.Record) bool { return accept(r, false) }
	finish := func(records []memory.Record, recent bool) (InputObservation, error) {
		now := uptime()
		s, err := inputAfterLookup(records, e, after, now)
		if err == nil {
			n.inputWait.reset()
			return s, nil
		}
		if recent && len(records) > 0 {
			// inputAfterLookup intentionally discards stale results. Decode with
			// the same validator again to retain only a bounded scheduling hint.
			s, invalid := inputObservation(records[0], e, after, now)
			if errors.Is(invalid, errInputStateStale) && s.SampleMillis >= now-inputRecentHintMillis && (n.inputHintRuntime != e.Runtime || s.SampleMillis > n.inputHintMillis) {
				n.inputHintRuntime, n.inputHintMillis = e.Runtime, s.SampleMillis
				if n.Hints == nil && !n.inputWait.stale(key, clock()) {
					return InputObservation{}, ErrPending
				}
				return InputObservation{}, ErrInputObservationStale
			}
			return InputObservation{}, ErrPending
		}
		if errors.Is(err, ErrInputObservationStale) && !n.inputWait.stale(key, clock()) {
			return InputObservation{}, ErrPending
		}
		return s, err
	}
	// Cache-off remains a complete scan path. No hidden address cache is
	// introduced to make one cadence work only when hints are enabled.
	if n.Hints != nil && len(n.Hints.Entries) > 0 {
		found, err := n.findPath(ctx, source, selector, true, true)
		if err != nil && !localLookupMiss(ctx, err) {
			return InputObservation{}, err
		}
		if len(found.Records) > 0 {
			return finish(found.Records, false)
		}
		if stale.Load() && n.inputWait.stale(key, clock()) || n.inputWait.waiting(key, clock()) {
			return InputObservation{}, ErrInputObservationStale
		}
	}
	selector.Accept = func(r memory.Record) bool { return accept(r, true) }
	found, err := n.findPath(ctx, source, selector, true, false)
	if err != nil {
		return InputObservation{}, err
	}
	if n.Hints == nil && len(found.Records) == 0 {
		// Only time metadata survives this call; no old address or payload is
		// retained under --no-cache. Recheck age after scanner drain/verification.
		tick, now := recentStaleMillis.Load(), uptime()
		if tick >= 0 && tick <= now+100 && tick >= now-inputRecentHintMillis && (n.inputHintRuntime != e.Runtime || tick > n.inputHintMillis) {
			n.inputHintRuntime, n.inputHintMillis = e.Runtime, tick
			if n.inputWait.stale(key, clock()) {
				return InputObservation{}, ErrInputObservationStale
			}
		}
	}
	if len(found.Records) == 0 && stale.Load() && n.Hints != nil && n.inputWait.stale(key, clock()) {
		return InputObservation{}, ErrInputObservationStale
	}
	return finish(found.Records, true)
}

// Input performs one already-journaled effect. No loop, journal callback or
// business transition lives here. The address is revalidated, never trusted.
func (n *Native) Input(ctx context.Context, a InputAction) (out InputOutcome, err error) {
	ctx, stopObservation := n.observationContext(ctx)
	defer stopObservation()
	if err := inputCapabilityError(a.Capability); err != nil {
		return InputOutcome{Disposition: "not_sent", Reason: "input_capability_unsupported"}, err
	}
	if a.Capability == bridge.InputSignalCapability {
		if a.Observation == nil {
			return InputOutcome{Disposition: "not_sent", Reason: "input_signal_unavailable", Retryable: true}, ErrPending
		}
		if err := n.recheckSignal(a.Envelope.Runtime, *a.Observation); err != nil {
			return InputOutcome{Disposition: "not_sent", Reason: "input_signal_unavailable", Retryable: true}, err
		}
	}
	if n.Guard == nil {
		return InputOutcome{Disposition: "not_sent", Reason: "guard_missing"}, errors.New("live.channel_guard_required")
	}
	// Resume may enter with a published transaction but no OS lock. Reacquire
	// the shared publication lease and verify the exact bytes BEFORE any key.
	if err := n.lockPublication(ctx); err != nil {
		return InputOutcome{Disposition: "not_sent", Reason: "shared_publication", Retryable: errors.Is(err, ErrPending)}, err
	}
	defer func() { err = errors.Join(err, n.unlockPublication()) }()
	slotAction := a.Kind == "invoke" || a.Kind == "escape" && a.Envelope.Action != "reload"
	if slotAction {
		if err := delivery.VerifySlotPublication(ctx, n.Parent, n.Version, n.Consumer, a.Envelope); err != nil {
			return InputOutcome{Disposition: "not_sent", Reason: "slot_publication_changed"}, err
		}
		if err := delivery.VerifySlotRoute(ctx, n.Parent, a.Envelope); err != nil {
			return InputOutcome{Disposition: "not_sent", Reason: "slot_route_changed"}, err
		}
	} else {
		// Reload loads no slot payload. Validate the pool under its own lock,
		// then release it before the physical burst or runtime readiness wait.
		_, err := delivery.InspectSlots(ctx, n.Parent, n.Version)
		closeErr := n.Publication.Close()
		n.Publication = nil
		if err = errors.Join(err, closeErr); err != nil {
			return InputOutcome{Disposition: "not_sent", Reason: "slot_pool_invalid"}, err
		}
	}
	receipt, err := desktop.WithReceiverInputProfile(ctx, n.Target, desktop.SlotReceiverBindings(), n.Guard, func(in *desktop.ReceiverInput) error {
		if a.Kind == "reload_fallback" {
			return in.FixedReload(func(int) error { return nil })
		}
		if a.Kind != "invoke" && a.Kind != "escape" && a.Kind != "reload" {
			return errors.New("live.channel_input_action_invalid")
		}
		if a.Observation == nil || a.Observation.Address == 0 {
			return ErrPending
		}
		runtime, _ := tokenBytes(a.Envelope.Runtime)
		r, readErr := memory.ReadRecord(ctx, n.memorySource(n.Process), a.Observation.Address, memory.Selector{Runtime: runtime, Nonce: runtime, Kind: bridge.MemoryInputState})
		if readErr != nil {
			if errors.Is(readErr, memory.ErrBudget) || ctx.Err() != nil {
				return readErr
			}
			return ErrPending
		}
		s, readErr := inputObservation(r, a.Envelope, 0, uptimeMillis())
		if readErr != nil || s.SampleMillis != a.Observation.SampleMillis {
			return ErrPending
		}
		if a.Capability == bridge.InputSignalCapability {
			s.Optical = a.Observation.Optical
			if err := n.recheckSignal(a.Envelope.Runtime, s); err != nil {
				return err
			}
		}
		if a.Kind == "escape" {
			if !*s.InputBlocked || s.Reason != "input_keyboard_focus" {
				return ErrPending
			}
			return in.Escape()
		}
		if *s.InputBlocked {
			return ErrPending
		}
		if a.Kind == "reload" {
			return in.ReadyReload()
		}
		return in.Wake()
	})
	out = InputOutcome{MessagesQueued: receipt.MessagesQueued, AtMillis: uptimeMillis(), Disposition: "uncertain"}
	if err == nil && receipt.SubmissionComplete {
		out.Disposition = "submitted"
	} else if receipt.MessagesQueued == 0 {
		out.Disposition = "not_sent"
		out.Retryable = errors.Is(err, desktop.ErrKeyAlreadyHeld) || errors.Is(err, ErrPending)
	}
	if err != nil {
		out.Reason = err.Error()
		var signalErr *inputSignalPending
		if errors.As(err, &signalErr) {
			out.Reason = signalErr.reason
		}
	}
	if out.Disposition != "not_sent" && n.inputSignal != nil {
		n.inputSignal.afterInput()
	}
	return out, err
}

func (n *Native) RuntimeCandidate(ctx context.Context, from Identity) (*Identity, error) {
	candidates, _, err := n.Discover(ctx, from.Character, from.Realm)
	if err != nil {
		return nil, err
	}
	for _, candidate := range candidates {
		if candidate.Runtime > from.Runtime && candidate.Owner == "" && candidate.GUID == from.GUID && candidate.Build == from.Build && candidate.Product == from.Product && candidate.Release == from.Release {
			return &candidate, nil
		}
	}
	return nil, nil
}
