package bridge

import (
	"context"
	"errors"
	"testing"

	"github.com/follenfang/lycheedev/internal/desktop"
)

func TestIdentityReportsCorrelatedReleaseMismatch(t *testing.T) {
	signal := identitySignal()
	signal.Release = "2.0.1"
	expected := SignalExpectation{Kind: "identity", Release: "2.0.2", ProbeNonce: signal.ProbeNonce, Product: signal.Product, Build: signal.Build}
	reader := ObserveSignals(&queuedFrames{frames: []*desktop.CapturedFrame{signalFrame(t, signal, 1)}, end: errors.New("ended")})
	got, err := reader.DiscoverIdentity(context.Background(), expected)
	if got.Release != "2.0.1" || err == nil || err.Error() != "bridge.runtime_release_mismatch: observed=2.0.1 expected=2.0.2" {
		t.Fatalf("lost observed runtime version: %+v %v", got, err)
	}
}
