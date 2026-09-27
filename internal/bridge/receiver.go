package bridge

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"time"

	"github.com/follenfang/lycheedev/internal/desktop"
)

// FrameFeed is supplied by a native WGC stream or a deterministic protocol test.
// The receiver never creates a window or changes capture modes on failure.
type FrameFeed interface {
	Next(context.Context) (*desktop.CapturedFrame, error)
}

type SignalReader struct {
	feed       FrameFeed
	lastTicks  int64
	observedAt time.Time
	baseline   SignalIdentity
	after      time.Time
}

// RuntimeReleaseMismatch is diagnostic identity evidence, never a session.
type RuntimeReleaseMismatch struct{ Observed, Expected string }

func (e *RuntimeReleaseMismatch) Error() string {
	return fmt.Sprintf("bridge.runtime_release_mismatch: observed=%s expected=%s", e.Observed, e.Expected)
}

func ObserveSignals(feed FrameFeed) *SignalReader { return &SignalReader{feed: feed} }

// SetObservationFence excludes captures observed before a native input step.
// A frame fence supplements, but does not replace, sequence/nonce correlation:
// the same old QR can still be drawn in a newer WGC frame.
func (r *SignalReader) SetObservationFence(after time.Time) {
	if r != nil {
		r.after = after
	}
}

// SetIdentityBaseline installs the actor and build identity a retained session
// already proved. Every later receipt on the wire may omit those fields to save
// QR modules, and the reader fills them from here before matching. It is
// replaced only when the owner adopts a freshly proved session.
func (r *SignalReader) SetIdentityBaseline(baseline SignalIdentity) {
	if r == nil {
		return
	}
	r.baseline = baseline
}

// RequireFreshSignal rechecks the age of the last successfully matched frame,
// including time spent decoding and persisting it. It grants no input permission
// and is invalidated by the next observation attempt, including a failed one.
func (r *SignalReader) RequireFreshSignal() error {
	if r == nil || r.observedAt.IsZero() {
		return errors.New("bridge.signal_not_observed")
	}
	// Three seconds still bounds the observed state to "the moment just
	// before this input" while tolerating the decode + persistence latency of
	// a loaded machine (shared CI runners, real game clients under disk
	// contention). Freshness is one layer of several input guards.
	age := time.Since(r.observedAt)
	if age < 0 || age > 3*time.Second {
		return errors.New("bridge.signal_expired")
	}
	return nil
}

func (r *SignalReader) WaitForSignal(ctx context.Context, expected SignalExpectation) (Signal, error) {
	r.observedAt = time.Time{}
	if expected.SessionNonce == "" || expected.Kind == "" || expected.Release == "" || expected.Character == "" || expected.Realm == "" || expected.Product == "" || expected.Build == "" {
		return Signal{}, errors.New("bridge.incomplete_signal_identity")
	}
	return r.waitForSignal(ctx, expected)
}

// WaitForReportedOutcome accepts a normal digest receipt or the addon's
// minimal structured report encoding failure for the same exact operation.
func (r *SignalReader) WaitForReportedOutcome(ctx context.Context, expected SignalExpectation) (Signal, error) {
	if expected.Kind != "reported" || expected.SessionNonce == "" || expected.RequestID == "" || expected.Release == "" || expected.Character == "" || expected.Realm == "" || expected.Product == "" || expected.Build == "" {
		return Signal{}, errors.New("bridge.incomplete_report_identity")
	}
	r.observedAt = time.Time{}
	expected.AllowReportError = true
	return r.waitForSignal(ctx, expected)
}

// DiscoverReady observes the first connection identity on an already selected
// window. Only ready discovery may omit nonce/actor filters; subsequent input
// and recovery continue to require the complete identity via WaitForSignal.
func (r *SignalReader) DiscoverReady(ctx context.Context, expected SignalExpectation) (Signal, error) {
	r.observedAt = time.Time{}
	if expected.Kind != "ready" || expected.Release == "" || expected.Product == "" || expected.Build == "" || !expected.RequireInputReady || expected.SessionNonce != "" || expected.RequestID != "" || expected.ReloadNonce != "" || expected.CleanupNonce != "" || expected.ProbeNonce != "" || expected.ActorState != "" || expected.AfterSequence != 0 || expected.RuntimeEpoch != 0 {
		return Signal{}, errors.New("bridge.invalid_discovery_identity")
	}
	return r.waitForSignal(ctx, expected)
}

// DiscoverIdentity observes one nonce-correlated identity marker on the already
// selected window. It binds no session and grants no input authority: probeNonce
// correlation replaces sequence freshness, and inputReady stays a display-time
// fact the caller may gate on. RequireInputReady selects the refreshed display.
func (r *SignalReader) DiscoverIdentity(ctx context.Context, expected SignalExpectation) (Signal, error) {
	r.observedAt = time.Time{}
	if expected.Kind != "identity" || expected.Release == "" || expected.Product == "" || expected.Build == "" ||
		!queueHex(expected.ProbeNonce, 32) ||
		(expected.ActorState != "" && expected.ActorState != "ok" && expected.ActorState != "no_actor" && expected.ActorState != "actor_restricted") ||
		expected.SessionNonce != "" || expected.RequestID != "" || expected.ReloadNonce != "" || expected.CleanupNonce != "" ||
		expected.AfterSequence != 0 || expected.RuntimeEpoch != 0 {
		return Signal{}, errors.New("bridge.invalid_discovery_identity")
	}
	return r.waitForSignal(ctx, expected)
}

// DiscoverReset observes the nonce-correlated receipt of the fixed bootstrap
// reset trigger on the already selected window. It binds no session, requires
// no input readiness, and grants no authority beyond the reported actor.
func (r *SignalReader) DiscoverReset(ctx context.Context, expected SignalExpectation) (Signal, error) {
	r.observedAt = time.Time{}
	if expected.Kind != "reset" || expected.Release == "" || expected.Product == "" || expected.Build == "" ||
		!queueHex(expected.ProbeNonce, 32) ||
		expected.SessionNonce != "" || expected.RequestID != "" || expected.ReloadNonce != "" || expected.CleanupNonce != "" ||
		expected.AfterSequence != 0 || expected.RuntimeEpoch != 0 || expected.RequireInputReady {
		return Signal{}, errors.New("bridge.invalid_discovery_reset")
	}
	return r.waitForSignal(ctx, expected)
}

// DiscoverReceiverReady observes a fresh, focused receiver independently of
// any bridge session. The caller should record the returned nonce and epoch
// before sending a frame; queued wake keys alone prove nothing.
func (r *SignalReader) DiscoverReceiverReady(ctx context.Context, expected SignalExpectation) (Signal, error) {
	r.observedAt = time.Time{}
	if expected.Kind != "receiver_ready" || expected.Release == "" || expected.Product == "" || expected.Build == "" || !expected.RequireInputReady || expected.SessionNonce != "" || expected.RequestID != "" || expected.ReceiverNonce != "" || expected.AttemptID != "" || expected.BodyAdler32 != "" || expected.CommitNonce != "" || expected.BodyBytes != 0 || expected.RequireAccepted {
		return Signal{}, errors.New("bridge.invalid_receiver_discovery")
	}
	return r.waitForSignal(ctx, expected)
}

// WaitForReceiverSignal requires exact nonce and stage identity. A rejected
// receipt can be matched by nonce alone because polluted input may never have
// contained a trustworthy request or attempt identifier.
func (r *SignalReader) WaitForReceiverSignal(ctx context.Context, expected SignalExpectation) (Signal, error) {
	r.observedAt = time.Time{}
	if expected.Release == "" || expected.Product == "" || expected.Build == "" || !queueHex(expected.ReceiverNonce, 32) || expected.RuntimeEpoch == 0 || expected.AfterSequence == 0 ||
		!strings.HasPrefix(expected.Kind, "receiver_") || expected.Kind == "receiver_ready" {
		return Signal{}, errors.New("bridge.invalid_receiver_expectation")
	}
	if expected.Kind == "receiver_staged" || expected.Kind == "receiver_commit_ready" || expected.Kind == "receiver_accepted" {
		if !queueHex(expected.AttemptID, 16) || !checksum(expected.BodyAdler32) || expected.BodyBytes == 0 || expected.RequestID == "" {
			return Signal{}, errors.New("bridge.invalid_receiver_expectation")
		}
	}
	if expected.Kind == "receiver_commit_ready" && !expected.RequireInputReady {
		return Signal{}, errors.New("bridge.invalid_receiver_expectation")
	}
	return r.waitForSignal(ctx, expected)
}

// WaitForReceiverStage returns either the exact staged/challenged readback or
// an explicit same-receiver rejection. Only rejected with accepted=false may
// justify a fresh attempt; cancellation and timeout still need recovery.
func (r *SignalReader) WaitForReceiverStage(ctx context.Context, expected SignalExpectation) (Signal, error) {
	if expected.Kind != "receiver_staged" && expected.Kind != "receiver_commit_ready" {
		return Signal{}, errors.New("bridge.invalid_receiver_stage_expectation")
	}
	if expected.Release == "" || expected.Product == "" || expected.Build == "" || !queueHex(expected.ReceiverNonce, 32) || expected.RuntimeEpoch == 0 || expected.AfterSequence == 0 || !queueHex(expected.AttemptID, 16) || !checksum(expected.BodyAdler32) || expected.BodyBytes == 0 || expected.RequestID == "" {
		return Signal{}, errors.New("bridge.invalid_receiver_stage_expectation")
	}
	r.observedAt = time.Time{}
	return r.waitForSignalWithFailures(ctx, expected, true)
}

func (r *SignalReader) waitForSignal(ctx context.Context, expected SignalExpectation) (Signal, error) {
	return r.waitForSignalWithFailures(ctx, expected, false)
}

func (r *SignalReader) waitForSignalWithFailures(ctx context.Context, expected SignalExpectation, failures bool) (Signal, error) {
	if r.feed == nil {
		return Signal{}, errors.New("bridge.missing_frame_feed")
	}
	for {
		if err := ctx.Err(); err != nil {
			return Signal{}, err
		}
		frame, err := r.feed.Next(ctx)
		if err != nil {
			return Signal{}, err
		}
		if frame == nil || frame.NRGBA == nil {
			return Signal{}, errors.New("bridge.invalid_frame")
		}
		age := time.Since(frame.ObservedAt)
		if frame.SystemTicks <= r.lastTicks || !r.after.IsZero() && !frame.ObservedAt.After(r.after) || age < 0 || age > time.Second {
			continue
		}
		r.lastTicks = frame.SystemTicks
		symbols, err := desktop.DecodeSymbols(frame.NRGBA)
		if err != nil {
			return Signal{}, err
		}
		var matching *Signal
		for _, text := range symbols {
			signals, err := ParseOpticalSignals(desktop.BytesFromSymbolText(text))
			if err != nil {
				continue
			}
			for _, signal := range signals {
				signal = FillSignalIdentity(signal, r.baseline)
				match := expected
				if expected.Kind == "identity" {
					match.Release = ""
				}
				if err := signal.Match(match); err != nil {
					if !failures || signal.Kind != "receiver_rejected" && signal.Kind != "receiver_cancelled" && signal.Kind != "receiver_timeout" {
						continue
					}
					failure := match
					failure.Kind = signal.Kind
					failure.RequestID, failure.AttemptID, failure.BodyAdler32, failure.CommitNonce = "", "", "", ""
					failure.BodyBytes, failure.RequireInputReady, failure.RequireAccepted = 0, false, false
					if signal.Match(failure) != nil {
						continue
					}
				}
				if matching != nil {
					return Signal{}, errors.New("bridge.ambiguous_signal")
				}
				matching = &signal
			}
		}
		if matching != nil {
			r.observedAt = frame.ObservedAt
			if err := r.RequireFreshSignal(); err != nil {
				r.observedAt = time.Time{}
				continue
			}
			if expected.Kind == "identity" && matching.Release != expected.Release {
				return *matching, &RuntimeReleaseMismatch{Observed: matching.Release, Expected: expected.Release}
			}
			return *matching, nil
		}
	}
}
