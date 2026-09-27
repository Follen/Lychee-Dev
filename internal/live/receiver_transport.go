package live

import (
	"context"
	"errors"
	"fmt"
	"image"
	"strings"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/buildinfo"
	"github.com/follenfang/lycheedev/internal/desktop"
)

var errReceiverRejected = errors.New("live.receiver_rejected")

type receiverProgress func(context.Context, string, bridge.ReceiverStage, int, bridge.Signal) error

type receiverBindingContextKey struct{}

func withReceiverBindings(ctx context.Context, bindings desktop.ReceiverBindings) context.Context {
	return context.WithValue(ctx, receiverBindingContextKey{}, bindings)
}

func receiverBindings(ctx context.Context) desktop.ReceiverBindings {
	if bindings, ok := ctx.Value(receiverBindingContextKey{}).(desktop.ReceiverBindings); ok && bindings.WakeBinding != "" {
		return bindings
	}
	return desktop.DefaultReceiverBindings()
}

type freshReceiverFeed struct {
	feed   sessionFrames
	after  time.Time
	beacon *startupBeaconObserver
}

func visibleReceiverReady(ctx context.Context, frames sessionFrames) (bridge.Signal, error) {
	pre, cancel := context.WithTimeout(ctx, 2*time.Second)
	defer cancel()
	for {
		frame, err := frames.Next(pre)
		if err != nil {
			return bridge.Signal{}, err
		}
		if frame == nil || frame.NRGBA == nil || frameHint(frame) != captureFrame {
			continue
		}
		symbols, err := desktop.DecodeSymbols(frame.NRGBA)
		if err != nil {
			return bridge.Signal{}, err
		}
		for _, symbol := range symbols {
			signals, err := bridge.ParseOpticalSignals(desktop.BytesFromSymbolText(symbol))
			if err != nil {
				continue
			}
			for _, signal := range signals {
				if signal.Kind == "receiver_ready" {
					return signal, nil
				}
			}
		}
		return bridge.Signal{}, nil
	}
}

func (f *freshReceiverFeed) Next(ctx context.Context) (*desktop.CapturedFrame, error) {
	for {
		frame, err := f.feed.Next(ctx)
		if err == nil && frame != nil && (f.after.IsZero() || frame.ObservedAt.After(f.after)) && f.beacon != nil && f.beacon.Observe(frame) {
			f.beacon = nil
			return nil, errReloadBeaconReady
		}
		if err != nil || frame == nil || f.after.IsZero() || frame.ObservedAt.After(f.after) {
			return frame, err
		}
	}
}

// parseBridgeCommand is a compatibility seam for the existing durable action
// preparers. It parses only the fixed internal verbs and never sends chat text.
func parseBridgeCommand(command string) (action, request, arg string, err error) {
	parts := strings.Fields(command)
	if len(parts) == 2 && parts[0] == "/dev" && parts[1] == "connect" {
		return "connect", "-", "-", nil
	}
	if len(parts) < 3 || parts[0] != "/dev" || parts[1] != "bridge" {
		return "", "", "", errors.New("live.receiver_action_invalid")
	}
	action = parts[2]
	switch action {
	case "identify", "reset", "refresh":
		if len(parts) == 4 {
			return action, "-", parts[3], nil
		}
	case "hide":
		if len(parts) == 3 {
			return action, "-", "-", nil
		}
	case "load", "run", "reload":
		if len(parts) == 4 {
			return action, parts[3], "-", nil
		}
	case "verify", "prepare", "clean", "flush", "ack", "bugs", "bugs-ack", "observe", "finish":
		if len(parts) == 5 {
			return action, parts[3], parts[4], nil
		}
	}
	return "", "", "", errors.New("live.receiver_action_invalid")
}

func receiverPreparedInput(target ClientWindow, region image.Rectangle, baseline bridge.SignalIdentity, progress receiverProgress) preparedInput {
	return func(ctx context.Context, window desktop.WindowIdentity, prepare func(context.Context) (string, error), guard func(context.Context) error) (desktop.InputReceipt, error) {
		if window != target.Window {
			return desktop.InputReceipt{}, errors.New("live.receiver_window_mismatch")
		}
		return sendReceiverInput(ctx, target, region, baseline, prepare, guard, progress)
	}
}

// sendReceiverInput holds one input lock across ready, staged and challenged
// acceptance. Only an explicit rejected receipt permits a new attempt; a lost
// accepted receipt leaves the original business intent unresolved.
func sendReceiverInput(ctx context.Context, target ClientWindow, region image.Rectangle, baseline bridge.SignalIdentity,
	prepare func(context.Context) (string, error), guard func(context.Context) error, progress receiverProgress,
) (desktop.InputReceipt, error) {
	if prepare == nil || guard == nil {
		return desktop.InputReceipt{}, errors.New("live.receiver_callbacks_required")
	}
	ctx, cancelTransaction := context.WithTimeout(ctx, receiverPhaseBudget)
	defer cancelTransaction()
	frames, err := desktop.CaptureFrames(ctx, target.Window, region)
	if err != nil {
		return desktop.InputReceipt{}, err
	}
	defer frames.Close()
	fresh := &freshReceiverFeed{feed: frames, beacon: &startupBeaconObserver{}}
	reader := bridge.ObserveSignals(fresh)
	reader.SetIdentityBaseline(baseline)
	var command string
	var action, request, arg string
	bindings := receiverBindings(ctx)
	return desktop.WithReceiverInputProfile(ctx, target.Window, bindings, guard, func(input *desktop.ReceiverInput) (runErr error) {
		closeKnown, commitRequested := false, false
		defer func() {
			if runErr != nil && ctx.Err() == nil && closeKnown && !commitRequested {
				// CLOSE only releases the receiver owned by this transaction. It
				// never replaces the original failure or claims business success.
				_ = input.Dismiss()
			}
		}()
		var err error
		command, err = prepare(ctx)
		if err != nil {
			return err
		}
		action, request, arg, err = parseBridgeCommand(command)
		if err != nil {
			return err
		}
		if err := preflightReceiverAction(action, request, arg); err != nil {
			return err
		}
		for attempt := 0; attempt < 3; attempt++ {
			attemptID, err := bridge.NewReceiverAttemptID()
			if err != nil {
				return err
			}
			stage := bridge.ReceiverStage{RequestID: request, AttemptID: attemptID, Action: action, Arg: arg}
			if err := guard(ctx); err != nil {
				return err
			}
			// A new capture of an old ready QR is not a wake receipt. Sample the
			// currently visible card before Wake and require a changed receiver
			// nonce afterward whenever a ready card was already displayed.
			before, err := visibleReceiverReady(ctx, frames)
			if err != nil {
				return err
			}
			if progress != nil {
				if err := progress(ctx, "wake_requested", stage, attempt, bridge.Signal{}); err != nil {
					return err
				}
			}
			fresh.after = time.Now()
			reader.SetObservationFence(time.Now())
			ready, err := awaitReceiverWake(ctx, input.Wake, func(wait context.Context) (bridge.Signal, error) { return receiverReady(wait, reader, target) })
			fresh.beacon = nil
			if err != nil {
				return err
			}
			if err := receiverWakeFresh(before, ready); err != nil {
				return err
			}
			effective, err := effectiveReceiverBindings(bindings, ready)
			if err != nil {
				return err
			}
			if err := input.SetBindings(effective); err != nil {
				return err
			}
			closeKnown = true
			bindings = effective
			if baseline.GUID != "" && ready.GUID != baseline.GUID || baseline.Character != "" && ready.Character != baseline.Character || baseline.Realm != "" && ready.Realm != baseline.Realm {
				return errors.New("live.receiver_actor_mismatch")
			}
			if err := validateReceiverReadyForAction(action, ready); err != nil {
				return err
			}
			if action == "observe" || action == "finish" {
				if baseline.RuntimeEpoch == 0 || ready.SessionNonce != baseline.SessionNonce || ready.PriorSessionEpoch < baseline.RuntimeEpoch || ready.PriorSessionEpoch > baseline.RuntimeEpoch+1 {
					return errors.New("live.receiver_observation_session_changed")
				}
			}
			if baseline.GUID == "" {
				reader.SetIdentityBaseline(bridge.SignalIdentity{Release: ready.Release, Product: ready.Product, Build: ready.Build,
					Character: ready.Character, Realm: ready.Realm, GUID: ready.GUID})
			}
			receive, stop := context.WithTimeout(ctx, 20*time.Second)
			defer stop()
			stage.ReceiverNonce, stage.RuntimeEpoch = ready.ReceiverNonce, ready.RuntimeEpoch
			wire, err := bridge.EncodeReceiverStage(stage)
			if err != nil {
				return err
			}
			body, err := stage.ReceiverBody()
			if err != nil {
				return err
			}
			digest, err := stage.BodyAdler32()
			if err != nil {
				return err
			}
			if progress != nil {
				if err := progress(ctx, "stage_requested", stage, attempt, ready); err != nil {
					return err
				}
			}
			reader.SetObservationFence(time.Now())
			if err := input.Stage(wire); err != nil {
				return err
			}
			expected := bridge.SignalExpectation{Kind: "receiver_staged", Release: buildinfo.Version,
				Product: target.Client.Product, Build: target.Client.FullBuild,
				ReceiverNonce: ready.ReceiverNonce, RuntimeEpoch: ready.RuntimeEpoch, RequestID: request,
				AttemptID: attemptID, BodyBytes: uint32(len(body)), BodyAdler32: digest, AfterSequence: ready.Sequence}
			staged, err := receiverStage(receive, reader, expected)
			if errors.Is(err, errReceiverRejected) {
				stop()
				if progress != nil {
					if e := progress(ctx, "rejected", stage, attempt, staged); e != nil {
						return e
					}
				}
				if !retryableReceiverCode(staged.ErrorCode) || attempt == 2 {
					return err
				}
				if err := receiverBackoff(ctx, attempt); err != nil {
					return err
				}
				continue
			}
			if err != nil {
				stop()
				return err
			}
			if ready.GUID != "" && staged.GUID != ready.GUID {
				stop()
				return errors.New("live.receiver_actor_mismatch")
			}
			if err := bridge.MatchReceiverReadback(stage, staged, "receiver_staged"); err != nil {
				return err
			}
			if progress != nil {
				if err := progress(ctx, "submit_requested", stage, attempt, staged); err != nil {
					return err
				}
			}
			reader.SetObservationFence(time.Now())
			if err := input.Submit(); err != nil {
				return err
			}
			expected.Kind, expected.AfterSequence, expected.RequireInputReady = "receiver_commit_ready", staged.Sequence, true
			challenge, err := receiverStage(receive, reader, expected)
			if errors.Is(err, errReceiverRejected) {
				stop()
				if progress != nil {
					if e := progress(ctx, "rejected", stage, attempt, challenge); e != nil {
						return e
					}
				}
				if !retryableReceiverCode(challenge.ErrorCode) || attempt == 2 {
					return err
				}
				if err := receiverBackoff(ctx, attempt); err != nil {
					return err
				}
				continue
			}
			if err != nil {
				stop()
				return err
			}
			if ready.GUID != "" && challenge.GUID != ready.GUID {
				stop()
				return errors.New("live.receiver_actor_mismatch")
			}
			commit, err := bridge.CommitForReceiverChallenge(stage, challenge)
			if err != nil {
				return err
			}
			commitWire, err := bridge.EncodeReceiverCommit(commit)
			if err != nil {
				return err
			}
			if progress != nil {
				if err := progress(ctx, "commit_requested", stage, attempt, challenge); err != nil {
					return err
				}
			}
			reader.SetObservationFence(time.Now())
			commitRequested = true
			if err := input.Commit(commitWire); err != nil {
				return err
			}
			// Submission is not acceptance or business success. The owning operation
			// observes its correlated result through its existing reader. Waiting
			// here for a transient accepted QR races that result and couples the
			// input lifetime to the business/UI lifetime.
			stop()
			return nil
		}
		return errReceiverRejected
	})
}

func awaitReceiverWake(ctx context.Context, wake func() error, observe func(context.Context) (bridge.Signal, error)) (bridge.Signal, error) {
	// A wake carries no business command. During loading it can be dropped;
	// while active it is idempotent and cannot extend the receiver deadline.
	// Keep these retries wholly before staging or requesting a commit.
	for attempt := 0; attempt < 3; attempt++ {
		if err := ctx.Err(); err != nil {
			return bridge.Signal{}, err
		}
		if err := wake(); err != nil {
			return bridge.Signal{}, err
		}
		wait, cancel := context.WithTimeout(ctx, 6*time.Second)
		ready, err := observe(wait)
		cancel()
		if err == nil || (!errors.Is(err, context.DeadlineExceeded) && !errors.Is(err, errReloadBeaconReady)) || ctx.Err() != nil || attempt == 2 {
			return ready, err
		}
	}
	return bridge.Signal{}, context.DeadlineExceeded
}

func validateReceiverReadyForAction(action string, ready bridge.Signal) error {
	if ready.ReceiverProtocol != "intent-v2" {
		return errors.New("live.receiver_protocol_mismatch: intent-v2 required")
	}
	if action == "connect" && ready.SessionNonce != "" && ready.PriorSessionEpoch == 0 {
		return errors.New("live.receiver_prior_session_capability_missing")
	}
	return nil
}

// Use the largest runtime epoch accepted by the wire grammar to check the
// worst-case frame length before Wake. The Lua-safe integer ceiling itself is
// reserved and rejected by both the Go encoder and the addon parser.
func preflightReceiverAction(action, request, arg string) error {
	stage := bridge.ReceiverStage{ReceiverNonce: strings.Repeat("0", 32), RuntimeEpoch: 9007199254740990,
		RequestID: request, AttemptID: strings.Repeat("0", 16), Action: action, Arg: arg}
	wire, err := bridge.EncodeReceiverStage(stage)
	if err != nil {
		return fmt.Errorf("live.receiver_preflight: %w", err)
	}
	if len(wire) > bridge.MaxHostReceiverStageBytes {
		return errors.New("live.receiver_wire_capacity")
	}
	return nil
}

func receiverWakeFresh(before, after bridge.Signal) error {
	if before.ReceiverNonce != "" && before.ReceiverNonce == after.ReceiverNonce {
		return errors.New("live.receiver_wake_not_observed")
	}
	return nil
}

func effectiveReceiverBindings(requested desktop.ReceiverBindings, ready bridge.Signal) (desktop.ReceiverBindings, error) {
	effective := desktop.ReceiverBindings{WakeBinding: ready.WakeBinding, SubmitBinding: ready.SubmitBinding, CloseBinding: ready.CloseBinding}
	if err := desktop.ValidateReceiverBindings(effective); err != nil {
		return effective, err
	}
	if effective.WakeBinding != requested.WakeBinding {
		return effective, errors.New("live.receiver_wake_binding_mismatch")
	}
	return effective, nil
}

func receiverReady(ctx context.Context, reader *bridge.SignalReader, target ClientWindow) (bridge.Signal, error) {
	wait, cancel := context.WithTimeout(ctx, 5*time.Second)
	defer cancel()
	return reader.DiscoverReceiverReady(wait, bridge.SignalExpectation{Kind: "receiver_ready", Release: buildinfo.Version,
		Product: target.Client.Product, Build: target.Client.FullBuild, RequireInputReady: true})
}

func receiverStage(ctx context.Context, reader *bridge.SignalReader, expected bridge.SignalExpectation) (bridge.Signal, error) {
	wait, cancel := context.WithTimeout(ctx, 5*time.Second)
	defer cancel()
	if expected.Kind == "receiver_staged" || expected.Kind == "receiver_commit_ready" {
		signal, err := reader.WaitForReceiverStage(wait, expected)
		if err != nil {
			return signal, err
		}
		switch signal.Kind {
		case expected.Kind:
			return signal, nil
		case "receiver_rejected":
			return signal, fmt.Errorf("%w: %s", errReceiverRejected, signal.ErrorCode)
		default:
			return signal, fmt.Errorf("live.receiver_%s: %s", signal.Kind, signal.ErrorCode)
		}
	}
	return reader.WaitForReceiverSignal(wait, expected)
}

func retryableReceiverCode(code string) bool {
	switch code {
	case "receiver_format", "receiver_encoding", "receiver_checksum", "receiver_length", "receiver_identity", "receiver_commit_mismatch":
		return true
	default:
		return false
	}
}

func receiverBackoff(ctx context.Context, attempt int) error {
	delay := 500 * time.Millisecond
	if attempt > 0 {
		delay = time.Second
	}
	timer := time.NewTimer(delay)
	defer timer.Stop()
	select {
	case <-ctx.Done():
		return ctx.Err()
	case <-timer.C:
		return nil
	}
}
