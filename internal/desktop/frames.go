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
	area   image.Rectangle
}

// CaptureArea is the actual window-relative region, resolved before capture starts.
func (s *FrameStream) CaptureArea() image.Rectangle { return s.area }

// WholeWindowCapture requests an explicit whole window, distinct from the zero
// automatic receipt region. It must be resolved against a real native extent;
// this marker is never a captured or persisted rectangle.
func WholeWindowCapture() image.Rectangle {
	return image.Rectangle{Min: image.Pt(-1, -1), Max: image.Pt(-1, -1)}
}

// InputSignalCapture requests client-relative (0,0)-(6,2), resolved natively
// against physical client and WGC frame bounds. It is not a screen rectangle.
func InputSignalCapture() image.Rectangle { return image.Rect(-2, -2, -2, -2) }

// ResolveCaptureArea bounds automatic receipt capture independently of monitor
// resolution. Explicit regions retain their coordinates and fail if clipped.
func ResolveCaptureArea(request image.Rectangle, extent image.Point) (image.Rectangle, error) {
	if extent.X <= 0 || extent.Y <= 0 || extent.X > 16384 || extent.Y > 16384 {
		return image.Rectangle{}, errors.New("desktop.invalid_capture_region")
	}
	if request == InputSignalCapture() {
		return image.Rectangle{}, errors.New("desktop.capture_client_geometry_required")
	}
	if request == (image.Rectangle{}) {
		return image.Rect(0, 0, min(extent.X, 1024), min(extent.Y, 1024)), nil
	}
	if request == WholeWindowCapture() {
		request = image.Rectangle{Max: extent}
	}
	if request.Empty() || request.Min.X < 0 || request.Min.Y < 0 || request.Dx() > 4096 || request.Dy() > 4096 {
		return image.Rectangle{}, errors.New("desktop.invalid_capture_region")
	}
	if request.Max.X > extent.X || request.Max.Y > extent.Y {
		return image.Rectangle{}, errors.New("desktop.capture_region_outside_window")
	}
	return request, nil
}

// resolveInputSignalArea accepts only a WGC extent matching measured physical
// visible bounds. No DPI scaling or title-bar offset is guessed.
func resolveInputSignalArea(extent image.Point, visible, client image.Rectangle) (image.Rectangle, error) {
	if visible.Empty() || client.Dx() < 6 || client.Dy() < 2 || visible.Size() != extent || !client.In(visible) {
		return image.Rectangle{}, errors.New("desktop.capture_client_geometry_mismatch")
	}
	origin := client.Min.Sub(visible.Min)
	return ResolveCaptureArea(image.Rectangle{Min: origin, Max: origin.Add(image.Pt(6, 2))}, extent)
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
