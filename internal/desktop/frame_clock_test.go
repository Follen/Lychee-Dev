package desktop

import (
	"context"
	"errors"
	"image"
	"testing"
	"time"
)

func TestInputSignalPhysicalClientMapping(t *testing.T) {
	for _, tc := range []struct {
		visible, client image.Rectangle
		want            image.Point
	}{
		{image.Rect(0, 0, 1920, 1080), image.Rect(0, 0, 1920, 1080), image.Pt(0, 0)},
		{image.Rect(-1600, 10, -600, 810), image.Rect(-1599, 41, -601, 809), image.Pt(1, 31)},
		{image.Rect(100, 100, 1600, 1300), image.Rect(102, 148, 1598, 1298), image.Pt(2, 48)},
	} {
		got, err := resolveInputSignalArea(tc.visible.Size(), tc.visible, tc.client)
		if err != nil || got.Min != tc.want || got.Size() != image.Pt(6, 2) {
			t.Fatal(got, err)
		}
	}
	v := image.Rect(0, 0, 100, 100)
	for _, c := range []image.Rectangle{image.Rect(-1, 0, 90, 90), image.Rect(0, 0, 5, 2), image.Rect(0, 0, 100, 101)} {
		if _, err := resolveInputSignalArea(v.Size(), v, c); err == nil {
			t.Fatal(c)
		}
	}
	if _, err := resolveInputSignalArea(image.Pt(101, 100), v, v); err == nil {
		t.Fatal("unverified WGC origin")
	}
	if _, err := ResolveCaptureArea(InputSignalCapture(), v.Size()); err == nil {
		t.Fatal("marker guessed client offset")
	}
}
func TestCaptureClockConversion(t *testing.T) {
	got, err := qpcToSystemTicks(123456789, 1000000)
	if err != nil || got != 1234567890 {
		t.Fatal(got, err)
	}
	for _, v := range [][2]int64{{-1, 1}, {1, 0}, {1, -1}, {9223372036854775807, 1}} {
		if _, err := qpcToSystemTicks(v[0], v[1]); err == nil {
			t.Fatal(v)
		}
	}
}
func TestFrameAgeUsesCaptureClock(t *testing.T) {
	frame := &CapturedFrame{SystemTicks: 10000000, ObservedAt: time.Now().Add(time.Hour)}
	age, err := FrameAge(frame, 15000000)
	if err != nil || age != 500*time.Millisecond {
		t.Fatal(age, err)
	}
	for _, v := range []int64{0, 9999999, 9223372036854775807} {
		if _, err := FrameAge(frame, v); err == nil {
			t.Fatal(v)
		}
	}
	if _, err := FrameAge(nil, 1); err == nil {
		t.Fatal("nil")
	}
}

func TestInputSignalFutureFrameWaitsWithoutChangingTimestamp(t *testing.T) {
	for _, tc := range []struct{ frame, now int64 }{{35259860164, 35259831701}, {35260901883, 35260861507}, {35262985274, 35262926024}} {
		frame := &CapturedFrame{SystemTicks: tc.frame}
		now := tc.now
		if _, err := FrameAge(frame, now); err == nil {
			t.Fatal("fixture no longer reproduces future frame")
		}
		waits := 0
		err := awaitFrameTime(context.Background(), frame.SystemTicks, func() (int64, error) { return now, nil }, func(ctx context.Context, d time.Duration) error {
			waits++
			if d < time.Duration(tc.frame-tc.now)*100*time.Nanosecond || d > 50*time.Millisecond {
				t.Fatal("wait outside bounded interval", d)
			}
			now += int64(d / (100 * time.Nanosecond))
			return nil
		})
		if err != nil || waits != 1 || frame.SystemTicks != tc.frame {
			t.Fatal(err, waits, frame)
		}
		if age, err := FrameAge(frame, now); err != nil || age < 0 {
			t.Fatal(age, err)
		}
	}
}
func TestInputSignalFutureFrameWaitFailsClosed(t *testing.T) {
	for _, tc := range []struct {
		name       string
		frame, now int64
		advance    bool
		wantWait   bool
	}{
		{"past", 10000, 20000, false, false}, {"equal", 20000, 20000, false, false},
		{"large", 600001, 100000, false, false}, {"zero", 0, 100000, false, false},
		{"frozen_clock", 120000, 100000, false, true},
	} {
		t.Run(tc.name, func(t *testing.T) {
			waits := 0
			err := awaitFrameTime(context.Background(), tc.frame, func() (int64, error) { return tc.now, nil }, func(context.Context, time.Duration) error { waits++; return nil })
			if (tc.name == "past" || tc.name == "equal") != (err == nil) {
				t.Fatal(err)
			}
			if (waits == 1) != tc.wantWait {
				t.Fatal(waits)
			}
		})
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if err := awaitFrameTime(ctx, 20000, func() (int64, error) { return 10000, nil }, func(context.Context, time.Duration) error { t.Fatal("cancelled wait ran"); return nil }); !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
	ctx, cancel = context.WithCancel(context.Background())
	defer cancel()
	err := awaitFrameTime(ctx, 20000, func() (int64, error) { return 10000, nil }, func(ctx context.Context, _ time.Duration) error { cancel(); return ctx.Err() })
	if !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
}
