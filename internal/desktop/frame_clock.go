package desktop

import (
	"context"
	"errors"
	"math"
	"time"
)

// awaitFrameTime handles a small observed producer/consumer timestamp lead.
// One timer, followed by a strict clock check; no timestamp rewriting, recurring
// polling, or relaxation of FrameAge. Anomalous leads fail closed.
func awaitFrameTime(ctx context.Context, ticks int64, clock func() (int64, error), wait func(context.Context, time.Duration) error) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	now, err := clock()
	if err != nil {
		return err
	}
	if ticks <= 0 || now < 0 {
		return errors.New("desktop.invalid_frame_time")
	}
	if ticks <= now {
		return nil
	}
	const maxLeadTicks int64 = 50 * 10000
	lead := ticks - now
	if lead > maxLeadTicks {
		return errors.New("desktop.capture_frame_time_lead")
	}
	// Round up to a millisecond to avoid an early wake from duration conversion.
	delay := time.Duration((lead+9999)/10000) * time.Millisecond
	if err := wait(ctx, delay); err != nil {
		return err
	}
	if err := ctx.Err(); err != nil {
		return err
	}
	now, err = clock()
	if err != nil {
		return err
	}
	if now < ticks {
		return errors.New("desktop.invalid_frame_time")
	}
	return nil
}

func waitCaptureFrameTime(ctx context.Context, ticks int64) error {
	return awaitFrameTime(ctx, ticks, CaptureSystemTicks, func(ctx context.Context, delay time.Duration) error {
		timer := time.NewTimer(delay)
		defer timer.Stop()
		select {
		case <-ctx.Done():
			return ctx.Err()
		case <-timer.C:
			return nil
		}
	})
}

// QPC timestamps and WGC SystemRelativeTime use the same epoch; the latter is
// expressed in 100-nanosecond units, not GetTickCount milliseconds.
func qpcToSystemTicks(counter, frequency int64) (int64, error) {
	if counter < 0 || frequency <= 0 {
		return 0, errors.New("desktop.invalid_qpc")
	}
	seconds, remainder := counter/frequency, counter%frequency
	if seconds > math.MaxInt64/10000000 || remainder > math.MaxInt64/10000000 {
		return 0, errors.New("desktop.qpc_overflow")
	}
	ticks := seconds*10000000 + remainder*10000000/frequency
	if ticks < 0 {
		return 0, errors.New("desktop.qpc_overflow")
	}
	return ticks, nil
}

// FrameAge compares WGC SystemRelativeTime with CaptureSystemTicks, both in
// 100ns QPC units. Wall time and frame delivery time are not freshness proof.
func FrameAge(frame *CapturedFrame, nowTicks int64) (time.Duration, error) {
	if frame == nil || frame.SystemTicks <= 0 || nowTicks < frame.SystemTicks {
		return 0, errors.New("desktop.invalid_frame_time")
	}
	delta := nowTicks - frame.SystemTicks
	if delta > math.MaxInt64/100 {
		return 0, errors.New("desktop.frame_age_overflow")
	}
	return time.Duration(delta) * 100 * time.Nanosecond, nil
}
