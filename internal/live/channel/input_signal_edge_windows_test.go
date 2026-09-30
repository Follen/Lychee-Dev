//go:build windows && amd64

package channel

import (
	"context"
	"errors"
	"image"
	"image/color"
	"sync/atomic"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/desktop"
)

type edgeFrameArrival struct {
	frame *desktop.CapturedFrame
	err   error
}

type edgeFrameSource struct {
	arrivals chan edgeFrameArrival
	next     chan struct{}
	closed   atomic.Bool
}

func (s *edgeFrameSource) Next(ctx context.Context) (*desktop.CapturedFrame, error) {
	s.next <- struct{}{}
	select {
	case <-ctx.Done():
		return nil, ctx.Err()
	case arrival := <-s.arrivals:
		return arrival.frame, arrival.err
	}
}

func (s *edgeFrameSource) Close() { s.closed.Store(true) }

func edgeSignalFixture(t *testing.T) (*nativeInputSignal, *edgeFrameSource) {
	t.Helper()
	source := &edgeFrameSource{arrivals: make(chan edgeFrameArrival), next: make(chan struct{}, 128)}
	signal := newNativeInputSignal(context.Background(), "runtime", source)
	t.Cleanup(signal.close)
	select {
	case <-source.next:
	case <-time.After(time.Second):
		t.Fatal("capture reader did not start")
	}
	return signal, source
}

func edgeSignalFrame(t *testing.T, state string, heartbeat bool, offset time.Duration) *desktop.CapturedFrame {
	t.Helper()
	green, white := color.NRGBA{G: 255, A: 255}, color.NRGBA{R: 255, G: 255, B: 255, A: 255}
	red, blue := color.NRGBA{R: 255, A: 255}, color.NRGBA{B: 255, A: 255}
	black := color.NRGBA{A: 255}
	cells := []color.NRGBA{green, white, black}
	if heartbeat {
		cells[2] = white
	}
	switch state {
	case "focus":
		cells[0], cells[1] = red, blue
	case "blocked":
		cells[0], cells[1] = blue, red
	case "invalid":
		cells[0], cells[1] = black, black
	}
	im := image.NewNRGBA(image.Rect(0, 0, 6, 2))
	for i, cell := range cells {
		for y := 0; y < 2; y++ {
			for x := i * 2; x < i*2+2; x++ {
				im.SetNRGBA(x, y, cell)
			}
		}
	}
	ticks, err := desktop.CaptureSystemTicks()
	if err != nil {
		t.Fatal(err)
	}
	return &desktop.CapturedFrame{NRGBA: im, SystemTicks: ticks + offset.Nanoseconds()/100}
}

func edgeFeed(t *testing.T, source *edgeFrameSource, arrival edgeFrameArrival) {
	t.Helper()
	select {
	case source.arrivals <- arrival:
	case <-time.After(time.Second):
		t.Fatal("capture arrival blocked")
	}
	if arrival.err == nil {
		select {
		case <-source.next: // The reader processed the previous frame.
		case <-time.After(time.Second):
			t.Fatal("capture reader stopped consuming events")
		}
	}
}

func edgeWait(signal *nativeInputSignal, ctx context.Context) <-chan error {
	result := make(chan error, 1)
	go func() {
		result <- signal.waitForNewEdge(ctx)
	}()
	return result
}

func edgeStillWaiting(t *testing.T, result <-chan error) {
	t.Helper()
	select {
	case err := <-result:
		t.Fatalf("capture-event wait completed before a new healthy edge: %v", err)
	case <-time.After(15 * time.Millisecond):
	}
}

func TestInputSignalWaitForAsynchronousNewEdge(t *testing.T) {
	signal, source := edgeSignalFixture(t)
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	result := edgeWait(signal, ctx)
	edgeStillWaiting(t, result)
	edgeFeed(t, source, edgeFrameArrival{frame: edgeSignalFrame(t, "ready", false, 0)})
	edgeStillWaiting(t, result)
	edgeFeed(t, source, edgeFrameArrival{frame: edgeSignalFrame(t, "ready", true, 0)})
	select {
	case err := <-result:
		if err != nil {
			t.Fatal(err)
		}
	case <-ctx.Done():
		t.Fatal("new edge did not wake existing capture-event waiter")
	}
	if source.closed.Load() {
		t.Fatal("successful scheduling wait closed the stream")
	}
}

func TestInputSignalWaitSameHeartbeatDoesNotComplete(t *testing.T) {
	signal, source := edgeSignalFixture(t)
	edgeFeed(t, source, edgeFrameArrival{frame: edgeSignalFrame(t, "ready", false, 0)})
	edgeFeed(t, source, edgeFrameArrival{frame: edgeSignalFrame(t, "ready", true, 0)})
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	result := edgeWait(signal, ctx)
	edgeStillWaiting(t, result)
	for i := 0; i < 3; i++ {
		edgeFeed(t, source, edgeFrameArrival{frame: edgeSignalFrame(t, "ready", true, 0)})
		edgeStillWaiting(t, result)
	}
	cancel()
	if err := <-result; !errors.Is(err, context.Canceled) || source.closed.Load() {
		t.Fatal("local cancellation changed stream lifetime", err)
	}
}

func TestInputSignalWaitRejectsUnhealthyArrivals(t *testing.T) {
	for _, kind := range []string{"invalid", "nil", "old", "future", "blocked"} {
		t.Run(kind, func(t *testing.T) {
			signal, source := edgeSignalFixture(t)
			ctx, cancel := context.WithTimeout(context.Background(), time.Second)
			defer cancel()
			result := edgeWait(signal, ctx)
			edgeStillWaiting(t, result)
			edgeFeed(t, source, edgeFrameArrival{frame: edgeSignalFrame(t, "ready", false, 0)})
			state, offset := "ready", time.Duration(0)
			switch kind {
			case "invalid":
				state = "invalid"
			case "old":
				offset = -501 * time.Millisecond
			case "future":
				offset = time.Second
			case "blocked":
				state = "blocked"
			}
			frame := edgeSignalFrame(t, state, true, offset)
			if kind == "nil" {
				frame = nil
			}
			edgeFeed(t, source, edgeFrameArrival{frame: frame})
			edgeStillWaiting(t, result)
			// Rejected captures cannot be redeemed without fresh observations.
			edgeFeed(t, source, edgeFrameArrival{frame: edgeSignalFrame(t, "ready", false, 0)})
			edgeFeed(t, source, edgeFrameArrival{frame: edgeSignalFrame(t, "ready", true, 0)})
			select {
			case err := <-result:
				if err != nil {
					t.Fatal(err)
				}
			case <-ctx.Done():
				t.Fatal("fresh capture edge did not recover event wait")
			}
		})
	}
}

func TestInputSignalWaitHonorsPostInputBarrier(t *testing.T) {
	signal, source := edgeSignalFixture(t)
	edgeFeed(t, source, edgeFrameArrival{frame: edgeSignalFrame(t, "ready", false, 0)})
	edgeFeed(t, source, edgeFrameArrival{frame: edgeSignalFrame(t, "ready", true, 0)})
	signal.afterInput()
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	result := edgeWait(signal, ctx)
	edgeStillWaiting(t, result)
	edgeFeed(t, source, edgeFrameArrival{frame: edgeSignalFrame(t, "ready", false, 0)})
	edgeStillWaiting(t, result)
	<-time.After(110 * time.Millisecond)
	edgeFeed(t, source, edgeFrameArrival{frame: edgeSignalFrame(t, "focus", true, 0)})
	select {
	case err := <-result:
		if err != nil {
			t.Fatal(err)
		}
	case <-ctx.Done():
		t.Fatal("valid edge past input barrier did not wake waiter")
	}
}

func TestInputSignalWaitCancellationDoesNotEndCapture(t *testing.T) {
	signal, source := edgeSignalFixture(t)
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Millisecond)
	defer cancel()
	if err := <-edgeWait(signal, ctx); !errors.Is(err, context.DeadlineExceeded) {
		t.Fatal("caller deadline was not retained", err)
	}
	signal.mu.Lock()
	streamErr := signal.err
	signal.mu.Unlock()
	if streamErr != nil || source.closed.Load() {
		t.Fatal("local wait timeout poisoned or closed stream", streamErr)
	}
	edgeFeed(t, source, edgeFrameArrival{frame: edgeSignalFrame(t, "ready", false, 0)})
	edgeFeed(t, source, edgeFrameArrival{frame: edgeSignalFrame(t, "ready", true, 0)})
	if _, err := signal.evidence(); err != nil {
		t.Fatal("capture did not survive local wait deadline", err)
	}
}

func TestInputSignalWaitStreamErrorWakes(t *testing.T) {
	signal, source := edgeSignalFixture(t)
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	result := edgeWait(signal, ctx)
	edgeStillWaiting(t, result)
	streamErr := errors.New("fixture.capture_ended")
	edgeFeed(t, source, edgeFrameArrival{err: streamErr})
	select {
	case err := <-result:
		if !errors.Is(err, streamErr) || !errors.Is(err, ErrPending) || errors.Is(err, context.Canceled) {
			t.Fatal("stream failure was lost or mislabeled cancellation", err)
		}
	case <-ctx.Done():
		t.Fatal("ended stream did not wake waiter")
	}
	if source.closed.Load() {
		t.Fatal("event waiter owned stream cleanup")
	}
}

func TestInputSignalCaptureNotificationsCoalesce(t *testing.T) {
	signal, source := edgeSignalFixture(t)
	for i := 0; i < 64; i++ {
		edgeFeed(t, source, edgeFrameArrival{frame: edgeSignalFrame(t, "ready", i%2 == 0, 0)})
	}
	if len(signal.arrival) != 1 || cap(signal.arrival) != 1 {
		t.Fatal("capture events were unbounded or not coalesced", len(signal.arrival), cap(signal.arrival))
	}
	if _, err := signal.evidence(); err != nil {
		t.Fatal("full notification channel blocked the capture reader", err)
	}
}

func TestInputPublicationWaitDoesNotInterchangeRuntime(t *testing.T) {
	signal, source := edgeSignalFixture(t)
	native := &Native{inputSignal: signal}
	if err := native.waitForInputPublication(context.Background(), "another-runtime"); !errors.Is(err, ErrPending) || native.inputSignal != signal || source.closed.Load() {
		t.Fatal("runtime mismatch replaced or closed active capture", err)
	}
	if err := (&Native{}).waitForInputPublication(context.Background(), "runtime"); !errors.Is(err, ErrPending) {
		t.Fatal("missing capture was treated as publication", err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if err := native.waitForInputPublication(ctx, "runtime"); !errors.Is(err, context.Canceled) || source.closed.Load() {
		t.Fatal("cancelled caller changed capture lifetime", err)
	}
}
