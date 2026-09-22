package bridge

import (
	"context"
	"errors"
	"image"
	"image/draw"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/desktop"
)

func TestIdentityDiscoveryCorrelatesProbeNonce(t *testing.T) {
	own := identitySignal()
	foreign := identitySignal()
	foreign.ProbeNonce = strings.Repeat("f", 32)
	ready := Signal{Schema: "lycheedev.signal.v1", Release: "2.0.0-dev", Kind: "ready", SessionNonce: strings.Repeat("a", 32), Character: "Paladin", Realm: "Realm", GUID: "Player-1-123", Product: "retail", Build: "12.1.0.69875", Sequence: 1, InputReady: true}
	expected := SignalExpectation{Kind: "identity", Release: own.Release, ProbeNonce: own.ProbeNonce, Product: own.Product, Build: own.Build}
	end := errors.New("no more frames")
	for _, mode := range []string{"valid", "filtered-actor", "stale-probe", "wrong-product", "ready-kind", "ready-display-only", "ambiguous", "not-identity-expectation", "incomplete", "session-expectation", "require-ready"} {
		t.Run(mode, func(t *testing.T) {
			candidate, want := own, expected
			switch mode {
			case "filtered-actor":
				want.ActorState = own.ActorState
			case "stale-probe":
				candidate = foreign
			case "wrong-product":
				candidate.Product = "classic"
			case "ready-kind":
				candidate.Kind, candidate.ProbeNonce, candidate.ActorState = "ready", "", ""
				candidate.SessionNonce, candidate.Sequence, candidate.Character, candidate.Realm = strings.Repeat("a", 32), 1, own.Character, own.Realm
			case "ready-display-only":
				candidate = ready
			case "not-identity-expectation":
				want.Kind = "ready"
				want.RequireInputReady = true
			case "incomplete":
				want.ProbeNonce = ""
			case "session-expectation":
				want.SessionNonce = strings.Repeat("a", 32)
			case "require-ready":
				want.RequireInputReady = true
				candidate.InputReady = false
				candidate.InputReason = "input_keyboard_focus"
			}
			frame := signalFrame(t, candidate, 1)
			if mode == "ambiguous" {
				// Two different receipts correlated to the same probe nonce on
				// one screen are still ambiguous: never pick a first code.
				right := own
				right.InputReady, right.InputReason = false, "input_keyboard_focus"
				pixels := image.NewNRGBA(image.Rect(0, 0, 1200, 600))
				draw.Draw(pixels, image.Rect(0, 0, 600, 600), frame.NRGBA, image.Point{}, draw.Src)
				draw.Draw(pixels, image.Rect(600, 0, 1200, 600), signalFrame(t, right, 1).NRGBA, image.Point{}, draw.Src)
				frame.NRGBA = pixels
			}
			reader := ObserveSignals(&queuedFrames{frames: []*desktop.CapturedFrame{frame}, end: end})
			got, err := reader.DiscoverIdentity(context.Background(), want)
			accepted := mode == "valid" || mode == "filtered-actor"
			if accepted {
				if err != nil || got != candidate {
					t.Fatalf("identity discovery: %+v %v", got, err)
				}
			} else if err == nil || reader.RequireFreshSignal() == nil {
				t.Fatalf("identity discovery accepted %s", mode)
			}
			if mode == "ambiguous" && err.Error() != "bridge.ambiguous_signal" {
				t.Fatal("did not reject duplicated receipts", err)
			}
		})
	}
}

func TestIdentityDiscoveryWaitsForTheRefreshedDisplay(t *testing.T) {
	unready := identitySignal()
	unready.InputReady, unready.InputReason = false, "input_keyboard_focus"
	refreshed := identitySignal()
	base := SignalExpectation{Kind: "identity", Release: refreshed.Release, ProbeNonce: refreshed.ProbeNonce, Product: refreshed.Product, Build: refreshed.Build}
	frames := []*desktop.CapturedFrame{
		signalFrame(t, unready, 1),
		signalFrame(t, unready, 2), // lingering first display in a newer frame
		signalFrame(t, refreshed, 3),
	}
	reader := ObserveSignals(&queuedFrames{frames: frames, end: errors.New("ended")})
	first, err := reader.DiscoverIdentity(context.Background(), base)
	if err != nil || first.InputReady {
		t.Fatalf("first display: %+v %v", first, err)
	}
	gate := base
	gate.RequireInputReady = true
	got, err := reader.DiscoverIdentity(context.Background(), gate)
	if err != nil || !got.InputReady || got.ProbeNonce != refreshed.ProbeNonce {
		t.Fatalf("refreshed display: %+v %v", got, err)
	}
	if err := reader.RequireFreshSignal(); err != nil {
		t.Fatal(err)
	}
}

func TestIdentityDiscoveryRejectsInvalidExpectations(t *testing.T) {
	reader := ObserveSignals(&queuedFrames{end: errors.New("ended")})
	base := SignalExpectation{Kind: "identity", Release: "2.0.0-dev", ProbeNonce: strings.Repeat("a", 32), Product: "retail", Build: "12.1.0.69875"}
	for _, mutate := range []func(*SignalExpectation){
		func(e *SignalExpectation) { e.Kind = "ready" },
		func(e *SignalExpectation) { e.Release = "" },
		func(e *SignalExpectation) { e.Product = "" },
		func(e *SignalExpectation) { e.Build = "" },
		func(e *SignalExpectation) { e.ProbeNonce = strings.ToUpper(e.ProbeNonce) },
		func(e *SignalExpectation) { e.ProbeNonce = "" },
		func(e *SignalExpectation) { e.ActorState = "loading" },
		func(e *SignalExpectation) { e.SessionNonce = strings.Repeat("b", 32) },
		func(e *SignalExpectation) { e.RequestID = "REQ-one" },
		func(e *SignalExpectation) { e.AfterSequence = 1 },
		func(e *SignalExpectation) { e.RuntimeEpoch = 1 },
	} {
		invalid := base
		mutate(&invalid)
		if _, err := reader.DiscoverIdentity(context.Background(), invalid); err == nil {
			t.Fatalf("accepted expectation %+v", invalid)
		}
	}
	invalid := SignalExpectation{Kind: "ready", Release: "2.0.0-dev", Product: "retail", Build: "12.1.0.69875", RequireInputReady: true, ProbeNonce: strings.Repeat("a", 32)}
	if _, err := reader.DiscoverReady(context.Background(), invalid); err == nil {
		t.Fatal("ready discovery accepted identity filters")
	}
}
