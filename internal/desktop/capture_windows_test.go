//go:build windows && amd64

package desktop

import (
	"context"
	"image"
	"os"
	"runtime"
	"strconv"
	"testing"
	"time"
)

// The default suite never touches arbitrary windows. This opt-in diagnostic
// validates native WGC item creation only; it does not claim frame capture.
func TestCaptureItemForExplicitProcess(t *testing.T) {
	requested := os.Getenv("LYCHEEDEV_TEST_CAPTURE_PID")
	if requested == "" {
		t.Skip("requires an explicitly authorized process ID")
	}
	pid, err := strconv.ParseUint(requested, 10, 32)
	if err != nil {
		t.Fatal(err)
	}
	windows, err := ListWindows(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	var target *WindowIdentity
	for i := range windows {
		if windows[i].ProcessID == uint32(pid) {
			if target != nil {
				t.Fatal("ambiguous process windows")
			}
			target = &windows[i]
		}
	}
	if target == nil {
		t.Fatal("selected process has no visible window")
	}
	if err := ConfirmWindow(context.Background(), *target); err != nil {
		t.Fatal(err)
	}
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()
	code, _, _ := initializeRuntime.Call(1)
	if err := runtimeFailure("initialize_runtime", code); err != nil {
		t.Fatal(err)
	}
	defer finishRuntime.Call()
	item, size, err := openCaptureItem(uintptr(target.Handle))
	if err != nil {
		t.Fatal(err)
	}
	defer item.release()
	t.Logf("WGC item PID=%d HWND=%d extent=%dx%d", target.ProcessID, target.Handle, size.Width, size.Height)
}

func TestCaptureFramesForExplicitProcess(t *testing.T) {
	requested := os.Getenv("LYCHEEDEV_TEST_CAPTURE_PID")
	if requested == "" {
		t.Skip("requires an explicitly authorized process ID")
	}
	pid, err := strconv.ParseUint(requested, 10, 32)
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	windows, err := ListWindows(ctx)
	if err != nil {
		t.Fatal(err)
	}
	var target *WindowIdentity
	for i := range windows {
		if windows[i].ProcessID == uint32(pid) {
			if target != nil {
				t.Fatal("ambiguous process windows")
			}
			target = &windows[i]
		}
	}
	if target == nil {
		t.Fatal("selected process has no visible window")
	}
	for iteration := 0; iteration < 3; iteration++ {
		stream, err := CaptureFrames(ctx, *target, image.Rect(100, 100, 740, 580))
		if err != nil {
			t.Fatal(err)
		}
		defer stream.Close()
		count := 0
		for count < 3 {
			select {
			case frame, ok := <-stream.Frames:
				if !ok {
					t.Fatal("capture ended without enough frames")
				}
				if frame.Bounds() != image.Rect(0, 0, 640, 480) {
					t.Fatal(frame.Bounds())
				}
				count++
			case err := <-stream.Errors:
				if err != nil {
					t.Fatal(err)
				}
			case <-ctx.Done():
				t.Fatal(ctx.Err())
			}
		}
		stream.Close()
		t.Logf("WGC stream %d received %d ROI frames and closed", iteration+1, count)
	}
	retained := 0
	activeNotices.Range(func(_, _ any) bool { retained++; return true })
	if retained != 0 {
		t.Fatalf("retained capture delegates: %d", retained)
	}
}
