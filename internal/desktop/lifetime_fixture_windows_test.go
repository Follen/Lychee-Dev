//go:build windows && amd64

package desktop

import (
	"context"
	"image"
	"os"
	"testing"
	"time"

	"golang.org/x/sys/windows"
)

func TestWGCLifetimeBetweenSessions(t *testing.T) {
	if os.Getenv("LYCHEEDEV_TEST_DESKTOP") != "1" {
		t.Skip("requires an interactive desktop for native WGC")
	}
	f := openFixtureWindow(t, image.NewNRGBA(image.Rect(0, 0, 160, 120)))
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()
	module, err := windows.UTF16PtrFromString("GraphicsCapture.dll")
	if err != nil {
		t.Fatal(err)
	}
	for i := 0; i < 10; i++ {
		stream, err := CaptureFrames(ctx, f.identity, image.Rectangle{})
		if err != nil {
			t.Fatal(err)
		}
		frame, err := stream.Next(ctx)
		stream.Close()
		if err != nil {
			t.Fatal(err)
		}
		if frame == nil || frame.Bounds().Empty() {
			t.Fatal("missing captured frame")
		}
		// FrameStream.Close releases capture resources, but must not tear down
		// the process MTA while WinRT's native workers can still be returning.
		// UNCHANGED_REFCOUNT only observes; it cannot keep the module loaded.
		var handle windows.Handle
		if err := windows.GetModuleHandleEx(windows.GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT, module, &handle); err != nil {
			t.Fatalf("capture runtime unloaded between sessions at cycle %d: %v", i, err)
		}
		if entries := countNotices(); entries != 0 {
			t.Fatalf("capture delegate leak: %d", entries)
		}
		timer := time.NewTimer(100 * time.Millisecond)
		select {
		case <-timer.C:
		case <-ctx.Done():
			timer.Stop()
			t.Fatal(ctx.Err())
		}
	}
}

func countNotices() int {
	count := 0
	activeNotices.Range(func(_, _ any) bool { count++; return true })
	return count
}
