//go:build windows && amd64

package channel

import (
	"context"
	"errors"
	"sync"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/desktop"
)

type inputFrameSource interface {
	Next(context.Context) (*desktop.CapturedFrame, error)
	Close()
}

// One command-owned stream, one bounded latest observation. It consumes WGC
// arrival events even during a slow memory lookup; no screenshots or timers.
type nativeInputSignal struct {
	runtime string
	stream  inputFrameSource
	cancel  context.CancelFunc
	done    chan struct{}
	arrival chan struct{}
	mu      sync.Mutex
	tracker inputSignalTracker
	err     error
}

func newNativeInputSignal(parent context.Context, runtime string, stream inputFrameSource) *nativeInputSignal {
	ctx, cancel := context.WithCancel(parent)
	s := &nativeInputSignal{runtime: runtime, stream: stream, cancel: cancel, done: make(chan struct{}), arrival: make(chan struct{}, 1)}
	go func() {
		defer close(s.done)
		defer s.notifyArrival()
		for {
			frame, err := stream.Next(ctx)
			if err != nil {
				s.mu.Lock()
				s.tracker.invalidate()
				s.err = err
				s.mu.Unlock()
				s.notifyArrival()
				return
			}
			if frame == nil {
				s.mu.Lock()
				s.tracker.invalidate()
				s.mu.Unlock()
				s.notifyArrival()
				continue
			}
			signal, decodeErr := bridge.DecodeInputSignal(frame.NRGBA)
			now, clockErr := desktop.CaptureSystemTicks()
			s.mu.Lock()
			if decodeErr != nil || clockErr != nil {
				s.tracker.rejectFrame(frame.SystemTicks, now)
			} else {
				s.tracker.accept(signal, frame.SystemTicks, now)
			}
			s.mu.Unlock()
			s.notifyArrival()
		}
	}()
	return s
}

func (s *nativeInputSignal) notifyArrival() {
	select {
	case s.arrival <- struct{}{}:
	default:
	}
}

// waitForNewEdge is a scheduling signal only. The caller owns its deadline;
// capture events coalesce into one notification and never block the reader.
// A previous edge cannot complete this wait or authorize memory/input work.
func (s *nativeInputSignal) waitForNewEdge(ctx context.Context) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	s.mu.Lock()
	baseline, streamErr := s.tracker.current.EdgeTicks, s.err
	s.mu.Unlock()
	if streamErr != nil {
		return errors.Join(&inputSignalPending{"input_signal_unavailable"}, streamErr)
	}
	for {
		select {
		case <-ctx.Done():
			return ctx.Err()
		case <-s.arrival:
		case <-s.done:
		}
		if err := ctx.Err(); err != nil {
			return err
		}
		observation, err := s.evidence()
		if err == nil && observation.EdgeTicks > baseline {
			return nil
		}
		s.mu.Lock()
		streamErr = s.err
		s.mu.Unlock()
		if streamErr != nil {
			return errors.Join(&inputSignalPending{"input_signal_unavailable"}, streamErr)
		}
	}
}

func (n *Native) waitForInputPublication(ctx context.Context, runtime string) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if n.inputSignal == nil || n.inputSignal.runtime != runtime {
		return &inputSignalPending{"input_signal_unavailable"}
	}
	return n.inputSignal.waitForNewEdge(ctx)
}

func (s *nativeInputSignal) close() {
	s.cancel()
	s.stream.Close()
	<-s.done
}

func (s *nativeInputSignal) evidence() (InputSignalEvidence, error) {
	now, err := desktop.CaptureSystemTicks()
	s.mu.Lock()
	defer s.mu.Unlock()
	if err != nil {
		s.tracker.invalidate()
		return InputSignalEvidence{}, errors.Join(ErrPending, err)
	}
	if s.err != nil {
		return InputSignalEvidence{}, errors.Join(&inputSignalPending{"input_signal_unavailable"}, s.err)
	}
	return s.tracker.evidence(now)
}

func (s *nativeInputSignal) afterInput() {
	now, err := desktop.CaptureSystemTicks()
	s.mu.Lock()
	defer s.mu.Unlock()
	if err != nil {
		s.tracker.invalidate()
		return
	}
	s.tracker.afterInput(now)
}

func (n *Native) observeSignal(ctx context.Context, runtime string) (InputSignalEvidence, error) {
	if n.inputSignal != nil && n.inputSignal.runtime != runtime {
		n.inputSignal.close()
		n.inputSignal = nil
	}
	if n.inputSignal == nil {
		stream, err := desktop.CaptureFramesWithStartupTimeout(ctx, n.Target, desktop.InputSignalCapture(), 2*time.Second)
		if err != nil {
			return InputSignalEvidence{}, errors.Join(&inputSignalPending{"input_signal_unavailable"}, err)
		}
		n.inputSignal = newNativeInputSignal(ctx, runtime, stream)
	}
	signal, err := n.inputSignal.evidence()
	if err != nil {
		n.inputSignal.mu.Lock()
		ended := n.inputSignal.err != nil
		n.inputSignal.mu.Unlock()
		if ended {
			n.inputSignal.close()
			n.inputSignal = nil
		}
	}
	return signal, err
}

func (n *Native) recheckSignal(runtime string, observation InputObservation) error {
	if n.inputSignal == nil || n.inputSignal.runtime != runtime || observation.Optical == nil {
		return &inputSignalPending{"input_signal_unavailable"}
	}
	signal, err := n.inputSignal.evidence()
	if err != nil {
		return err
	}
	if !inputSignalAgrees(signal, observation) {
		return &inputSignalPending{"input_signal_changed"}
	}
	return nil
}
