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
)

var inputUptime = windows.NewLazySystemDLL("kernel32.dll").NewProc("GetTickCount64")

func uptimeMillis() int64 { n, _, _ := inputUptime.Call(); return int64(n) }

func (n *Native) ObserveInput(ctx context.Context, e bridge.SlotEnvelope, after int64) (InputObservation, error) {
	if n.Guard == nil {
		return InputObservation{}, errors.New("live.channel_guard_required")
	}
	if err := n.Guard(ctx); err != nil {
		return InputObservation{}, err
	}
	runtime, _ := tokenBytes(e.Runtime)
	selector := memory.Selector{Runtime: runtime, Nonce: runtime, Kind: bridge.MemoryInputState}
	selector.Accept = func(r memory.Record) bool { _, err := inputObservation(r, e, after, uptimeMillis()); return err == nil }
	found, err := n.Find(ctx, selector, true)
	if err != nil {
		return InputObservation{}, err
	}
	if len(found.Records) == 0 {
		return InputObservation{}, ErrPending
	}
	s, err := inputObservation(found.Records[0], e, after, uptimeMillis())
	if err != nil {
		return InputObservation{}, ErrPending
	}
	s.Address = found.Records[0].Address
	return s, nil
}

// Input performs one already-journaled effect. No loop, journal callback or
// business transition lives here. The address is revalidated, never trusted.
func (n *Native) Input(ctx context.Context, a InputAction) (out InputOutcome, err error) {
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
		r, readErr := memory.ReadRecord(ctx, n.Process, a.Observation.Address, memory.Selector{Runtime: runtime, Nonce: runtime, Kind: bridge.MemoryInputState})
		if readErr != nil {
			return ErrPending
		}
		s, readErr := inputObservation(r, a.Envelope, 0, uptimeMillis())
		if readErr != nil || s.SampleMillis != a.Observation.SampleMillis {
			return ErrPending
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
