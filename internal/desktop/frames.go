package desktop

import (
	"context"
	"errors"
	"image"
	"sync"
	"time"
)

// FrameStream owns native capture until Close completes. Frames is a bounded
// latest-frame channel: dropped intermediate frames are not absence evidence.
type FrameStream struct {
	Frames <-chan *CapturedFrame
	Errors <-chan error
	cancel context.CancelFunc
	done   <-chan struct{}
	once   sync.Once
}

type CapturedFrame struct {
	*image.NRGBA
	SystemTicks int64
	ObservedAt  time.Time
}

func (s *FrameStream) Next(ctx context.Context) (*CapturedFrame, error) {
	select {
	case frame, ok := <-s.Frames:
		if ok {
			return frame, nil
		}
		select {
		case err := <-s.Errors:
			if err != nil {
				return nil, err
			}
		default:
		}
		return nil, errors.New("desktop.capture_closed")
	case err, ok := <-s.Errors:
		if ok && err != nil {
			return nil, err
		}
		return nil, errors.New("desktop.capture_closed")
	case <-ctx.Done():
		return nil, ctx.Err()
	}
}

func (s *FrameStream) Close() { s.once.Do(s.cancel); <-s.done }
