//go:build windows && amd64

package channel

import (
	"context"
	"errors"
	"sync/atomic"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/desktop"
)

type cancelledSignalFrames struct{ closed atomic.Bool }

func (s *cancelledSignalFrames) Next(ctx context.Context) (*desktop.CapturedFrame, error) {
	<-ctx.Done()
	return nil, ctx.Err()
}
func (s *cancelledSignalFrames) Close() { s.closed.Store(true) }

func TestNativeCloseStopsSignalWithoutHoldingPublication(t *testing.T) {
	stream := &cancelledSignalFrames{}
	n := &Native{inputSignal: newNativeInputSignal(context.Background(), "r", stream)}
	done := make(chan error, 1)
	go func() { done <- n.Close() }()
	select {
	case err := <-done:
		if err != nil {
			t.Fatal(err)
		}
	case <-time.After(time.Second):
		t.Fatal("capture lifetime leaked")
	}
	if !stream.closed.Load() || n.inputSignal != nil {
		t.Fatal("signal resources retained")
	}
	if err := n.Close(); err != nil {
		t.Fatal("repeat Close", err)
	}
}

func TestHybridInputMissingSignalIsProvenUnsent(t *testing.T) {
	n := &Native{}
	out, err := n.Input(context.Background(), InputAction{Kind: "invoke", Capability: bridge.InputSignalCapability, Observation: &InputObservation{}})
	if !errors.Is(err, ErrPending) || out.Disposition != "not_sent" || out.MessagesQueued != 0 || !out.Retryable {
		t.Fatal(out, err)
	}
	if n.Publication != nil {
		t.Fatal("input acquired publication before missing signal gate")
	}
}
