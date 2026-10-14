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
	"time"
)

var inputUptime = windows.NewLazySystemDLL("kernel32.dll").NewProc("GetTickCount64")

func uptimeMillis() int64 { n, _, _ := inputUptime.Call(); return int64(n) }

func (n *Native) ObserveInput(ctx context.Context, e bridge.SlotEnvelope, after int64, capability string) (InputObservation, error) {
	if err := inputActionCapabilityError(capability, e.Action); err != nil {
		return InputObservation{}, err
	}
	if n.Guard == nil {
		return InputObservation{}, errors.New("live.channel_guard_required")
	}
	if err := n.Guard(ctx); err != nil {
		return InputObservation{}, err
	}
	return n.observeInputFrom(ctx, e, after, uptimeMillis, time.Now)
}

func (n *Native) observeInputFrom(ctx context.Context, e bridge.SlotEnvelope, after int64, uptime func() int64, clock func() time.Time) (InputObservation, error) {
	runtime, _ := tokenBytes(e.Runtime)
	found, err := n.Find(ctx, memory.Selector{Runtime: runtime, Nonce: runtime, Kind: bridge.MemoryInputState}, true)
	if err != nil {
		return InputObservation{}, err
	}
	sample, err := inputAfterLookup(found.Records, e, after, uptime())
	if err == nil {
		n.inputWait.reset()
		return sample, nil
	}
	if errors.Is(err, ErrInputObservationStale) && !n.inputWait.stale(inputSampleKey(e, after), clock()) {
		return InputObservation{}, ErrPending
	}
	return sample, err
}

// Input performs one already-journaled effect. No loop, journal callback or
// business transition lives here. The address is revalidated, never trusted.
func (n *Native) Input(ctx context.Context, a InputAction) (out InputOutcome, err error) {
	if a.Kind != "invoke" && a.Kind != "reload" {
		return InputOutcome{Disposition: "not_sent", Reason: "input_action_retired"}, errors.New("live.channel_input_action_invalid")
	}
	if err := inputActionCapabilityError(a.Capability, a.Kind); err != nil {
		return InputOutcome{Disposition: "not_sent", Reason: "input_capability_unsupported"}, err
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
	slotAction := a.Kind == "invoke"
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
	if a.Observation == nil || !a.Observation.validBindings() || a.Observation.Bindings.Reload == "" {
		return InputOutcome{Disposition: "not_sent", Reason: "input_bindings_invalid"}, ErrPending
	}
	send := func(wake, reload func() error) error {
		if a.Observation == nil || a.Observation.Address == 0 {
			return ErrPending
		}
		runtime, _ := tokenBytes(a.Envelope.Runtime)
		if n.Mailbox == nil {
			return errors.New("live.channel_mailbox_required")
		}
		current, readErr := n.Find(ctx, memory.Selector{Runtime: runtime, Nonce: runtime, Kind: bridge.MemoryInputState}, true)
		if readErr != nil {
			if ctx.Err() != nil {
				return ctx.Err()
			}
			return readErr
		}
		if len(current.Records) != 1 || current.Records[0].Address != a.Observation.Address {
			return ErrPending
		}
		r := current.Records[0]
		s, readErr := inputObservation(r, a.Envelope, 0, uptimeMillis())
		if readErr != nil || s.SampleMillis != a.Observation.SampleMillis || !sameInputProfile(s, *a.Observation) {
			return ErrPending
		}

		if *s.InputBlocked {
			return ErrPending
		}
		if a.Kind == "reload" {
			return reload()
		}
		return wake()
	}
	profile, err := a.Observation.mailboxProfile(a.Kind)
	if err != nil {
		return InputOutcome{Disposition: "not_sent", Reason: err.Error()}, err
	}
	receipt, err := desktop.WithMailboxInput(ctx, n.Target, profile, n.Guard, send)
	out = InputOutcome{MessagesQueued: receipt.MessagesQueued, AtMillis: uptimeMillis(), Disposition: "uncertain"}
	if err == nil && receipt.SubmissionComplete {
		out.Disposition = "submitted"
	} else if receipt.MessagesQueued == 0 {
		out.Disposition = "not_sent"
		out.Retryable = errors.Is(err, desktop.ErrKeyAlreadyHeld) || errors.Is(err, ErrPending) || errors.Is(err, memory.ErrMailboxPathChanged)
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
