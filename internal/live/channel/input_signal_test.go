package channel

import (
	"context"
	"errors"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
)

func signalTicks(ms int64) int64 { return ms * signalTicksPerMillisecond }
func signalFeed(t *inputSignalTracker, state string, beat bool, ms int64) {
	t.accept(bridge.InputSignal{State: state, Heartbeat: beat}, signalTicks(ms), signalTicks(ms))
}

func TestSignalRequiresHeartbeatAndRejectsFrozenReady(t *testing.T) {
	var gate inputSignalTracker
	signalFeed(&gate, "ready", false, 1000)
	if _, err := gate.evidence(signalTicks(1000)); !errors.Is(err, ErrPending) {
		t.Fatal("first green authorized input", err)
	}
	signalFeed(&gate, "ready", true, 2000)
	if _, err := gate.evidence(signalTicks(2000)); err != nil {
		t.Fatal(err)
	}
	// Fresh compositor frames cannot keep a frozen Lua heartbeat alive.
	for ms := int64(2100); ms <= 3600; ms += 100 {
		signalFeed(&gate, "ready", true, ms)
	}
	if _, err := gate.evidence(signalTicks(3600)); !errors.Is(err, ErrPending) {
		t.Fatal("rendering extended Lua liveness", err)
	}
}

func TestSignalInvalidationRequiresNewEdgeAndNoOldFrame(t *testing.T) {
	var gate inputSignalTracker
	signalFeed(&gate, "ready", false, 1000)
	signalFeed(&gate, "ready", true, 2000)
	gate.invalidate() // Missing block, decode failure or resize.
	signalFeed(&gate, "ready", false, 2100)
	if _, err := gate.evidence(signalTicks(2100)); !errors.Is(err, ErrPending) {
		t.Fatal("old validity survived", err)
	}
	signalFeed(&gate, "ready", true, 3100)
	if _, err := gate.evidence(signalTicks(3100)); err != nil {
		t.Fatal(err)
	}
	if _, err := gate.evidence(signalTicks(3601)); !errors.Is(err, ErrPending) {
		t.Fatal("stale captured frame authorized", err)
	}
	signalFeed(&gate, "ready", false, 4100)
	if _, err := gate.evidence(signalTicks(4100)); !errors.Is(err, ErrPending) {
		t.Fatal("first resumed frame authorized", err)
	}
}

func TestSignalReordersFutureFramesAndLongGapsFailClosed(t *testing.T) {
	for _, kind := range []string{"repeat", "future", "old", "gap"} {
		t.Run(kind, func(t *testing.T) {
			var gate inputSignalTracker
			signalFeed(&gate, "ready", false, 1000)
			signalFeed(&gate, "ready", true, 2000)
			now := signalTicks(2100)
			switch kind {
			case "repeat":
				gate.accept(bridge.InputSignal{State: "ready", Heartbeat: false}, signalTicks(2000), now)
			case "future":
				gate.accept(bridge.InputSignal{State: "ready", Heartbeat: false}, signalTicks(2200), now)
			case "old":
				gate.accept(bridge.InputSignal{State: "ready", Heartbeat: false}, signalTicks(2001), signalTicks(2600))
			case "gap":
				signalFeed(&gate, "ready", false, 3600)
				now = signalTicks(3600)
			}
			if _, err := gate.evidence(now); !errors.Is(err, ErrPending) {
				t.Fatal("invalid capture authorized", err)
			}
		})
	}
}

func TestSignalPostInputBarrierCannotReuseEarlierEdge(t *testing.T) {
	var gate inputSignalTracker
	signalFeed(&gate, "ready", false, 1000)
	signalFeed(&gate, "ready", true, 2000)
	gate.afterInput(signalTicks(2050))
	signalFeed(&gate, "ready", false, 2100)
	if _, err := gate.evidence(signalTicks(2100)); !errors.Is(err, ErrPending) {
		t.Fatal("too-early edge authorized", err)
	}
	signalFeed(&gate, "ready", true, 3000)
	if _, err := gate.evidence(signalTicks(3000)); err != nil {
		t.Fatal(err)
	}
}

func TestSignalInvalidationDoesNotResetStreamReplayHighwater(t *testing.T) {
	var gate inputSignalTracker
	signalFeed(&gate, "ready", false, 1000)
	signalFeed(&gate, "ready", true, 2000)
	for _, ms := range []int64{2000, 1900, 2000} {
		gate.accept(bridge.InputSignal{State: "ready", Heartbeat: ms == 2000}, signalTicks(ms), signalTicks(2100))
		if _, err := gate.evidence(signalTicks(2100)); !errors.Is(err, ErrPending) {
			t.Fatal("replayed edge authorized after invalidation", ms, err)
		}
	}
}

func TestSignalBadColorFrameAdvancesReplayHighwater(t *testing.T) {
	var gate inputSignalTracker
	signalFeed(&gate, "ready", false, 1000)
	signalFeed(&gate, "ready", true, 2000)
	gate.rejectFrame(signalTicks(2400), signalTicks(2500))
	for _, ms := range []int64{2100, 2300} {
		gate.accept(bridge.InputSignal{State: "ready", Heartbeat: ms == 2300}, signalTicks(ms), signalTicks(2500))
		if _, err := gate.evidence(signalTicks(2500)); !errors.Is(err, ErrPending) {
			t.Fatal("replay before corrupt frame authorized", ms, err)
		}
	}
}

func TestHybridBlockedSignalNeverScansMemory(t *testing.T) {
	for _, state := range []string{"input_combat_lockdown", "unknown"} {
		t.Run(state, func(t *testing.T) {
			var gate inputSignalTracker
			signalFeed(&gate, state, false, 1000)
			signalFeed(&gate, state, true, 2000)
			_, err := observeHybridInput(func() (InputSignalEvidence, error) { return gate.evidence(signalTicks(2000)) }, func() (InputObservation, error) {
				t.Fatal("blocked signal caused memory lookup")
				return InputObservation{}, nil
			})
			if !errors.Is(err, ErrPending) {
				t.Fatal(err)
			}
		})
	}
}

func TestHybridRechecksAfterLookupAndRequiresAgreement(t *testing.T) {
	for _, state := range []string{"ready", "input_keyboard_focus", "unavailable"} {
		t.Run(state, func(t *testing.T) {
			reads := 0
			calls := 0
			blocked := false
			s, err := observeHybridInput(func() (InputSignalEvidence, error) {
				calls++
				if calls == 1 {
					return InputSignalEvidence{State: "ready"}, nil
				}
				if state == "unavailable" {
					return InputSignalEvidence{}, ErrPending
				}
				return InputSignalEvidence{State: state, FrameTicks: 123}, nil
			}, func() (InputObservation, error) {
				reads++
				return InputObservation{SampleMillis: 456, InputBlocked: &blocked}, nil
			})
			if reads != 1 || calls != 2 {
				t.Fatal("unexpected observation work", reads, calls)
			}
			if state == "ready" {
				if err != nil || s.Optical == nil || s.Optical.FrameTicks != 123 || s.SampleMillis != 456 {
					t.Fatal(s, err)
				}
			} else if !errors.Is(err, ErrPending) {
				t.Fatal("mismatch authorized", err)
			}
		})
	}
}

func TestHybridPreservesMemoryTargetAndFreshnessFailures(t *testing.T) {
	for _, failure := range []error{ErrInputObservationStale, errors.New("live.channel_input_target_changed")} {
		_, err := observeHybridInput(func() (InputSignalEvidence, error) { return InputSignalEvidence{State: "ready"}, nil }, func() (InputObservation, error) { return InputObservation{}, failure })
		if !errors.Is(err, failure) {
			t.Fatal("optical readiness overrode memory guard", err)
		}
	}
}

func TestInputCapabilityRouting(t *testing.T) {
	for _, capability := range []string{"lycheedev.input.v1", bridge.InputSignalCapability} {
		if !observedInputCapability(capability) || inputCapabilityError(capability) != nil {
			t.Fatal(capability)
		}
	}
	if observedInputCapability("") || observedInputCapability("future") || inputCapabilityError("future") == nil {
		t.Fatal("unknown telemetry could downgrade")
	}
}

func TestSignalLivenessLossDoesNotSuppressRuntimeDiscovery(t *testing.T) {
	for _, reason := range []string{"input_signal_unavailable", "input_signal_waiting", "input_signal_unknown"} {
		t.Run(reason, func(t *testing.T) {
			ctx, cancel := context.WithTimeout(context.Background(), time.Second)
			defer cancel()
			peer := &signalRecoveryPeer{reason: reason, stop: cancel}
			i := Identity{Runtime: strings.Repeat("1", 32), NextSlot: 1, Slots: 200, GUID: "g", Character: "c", Realm: "r", Build: "b", Product: "retail", Release: "3.0.0", InputState: bridge.InputSignalCapability}
			d, err := New(filepath.Join(t.TempDir(), "signal.jsonl"), peer, i)
			if err != nil {
				t.Fatal(err)
			}
			err = d.Continue(ctx)
			if !errors.Is(err, ErrPending) || peer.discoveries != 1 || peer.sends != 0 {
				t.Fatal("optical failure prevented recovery or caused input", err, peer.discoveries, peer.sends)
			}
		})
	}
}

type signalRecoveryPeer struct {
	pendingBackend
	reason      string
	stop        context.CancelFunc
	discoveries int
}

func (p *signalRecoveryPeer) ObserveInput(context.Context, bridge.SlotEnvelope, int64, string) (InputObservation, error) {
	return InputObservation{}, &inputSignalPending{p.reason}
}
func (p *signalRecoveryPeer) RuntimeCandidate(context.Context, Identity) (*Identity, error) {
	p.discoveries++
	p.stop()
	return nil, nil
}
