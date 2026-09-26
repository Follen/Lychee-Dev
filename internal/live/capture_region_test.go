package live

import (
	"context"
	"image"
	"testing"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/desktop"
)

type resolvedAreaFrames struct {
	sessionFrames
	area image.Rectangle
}

func (f *resolvedAreaFrames) CaptureArea() image.Rectangle { return f.area }

func TestConnectPersistsActualCaptureRegion(t *testing.T) {
	for _, requested := range []image.Rectangle{{}, image.Rect(12, 18, 640, 480), desktop.WholeWindowCapture()} {
		t.Run(requested.String(), func(t *testing.T) {
			extent := image.Pt(5120, 1440)
			if requested == desktop.WholeWindowCapture() {
				extent = image.Pt(1920, 1080)
			}
			client, fake, root := connectFixture(t)
			fake.windows = []desktop.WindowIdentity{testWindow(1, client)}
			fake.respond[1] = func(command string) []*desktop.CapturedFrame {
				if command == "/dev connect" {
					return []*desktop.CapturedFrame{fake.frame(1, readyReceipt(func(s *bridge.Signal) { s.InputReady = true }))}
				}
				return identityResponder(fake, 1, nil)(command)
			}
			io := fake.io()
			capture := io.capture
			var captureRequests []image.Rectangle
			io.capture = func(ctx context.Context, window desktop.WindowIdentity, region image.Rectangle) (sessionFrames, error) {
				captureRequests = append(captureRequests, region)
				actual, err := desktop.ResolveCaptureArea(region, extent)
				if err != nil {
					return nil, err
				}
				frames, err := capture(ctx, window, actual)
				if err != nil {
					return nil, err
				}
				return &resolvedAreaFrames{sessionFrames: frames, area: actual}, nil
			}
			connection, err := connectWindow(context.Background(), root, ConnectRequest{Snapshot: testPin(t, root), CaptureArea: requested}, io)
			if err != nil {
				t.Fatal(err)
			}
			saved, err := ReadWindowSession(context.Background(), root, connection.ID)
			if err != nil {
				t.Fatal(err)
			}
			want, _ := desktop.ResolveCaptureArea(requested, extent)
			if saved.Record.Region != want {
				t.Fatalf("persisted %v, want actual %v", saved.Record.Region, want)
			}
			if len(captureRequests) == 0 {
				t.Fatal("no capture")
			}
			// The saved concrete rectangle remains usable by the next capture; no
			// automatic policy marker or screen-size guess leaks into the session.
			again, err := desktop.ResolveCaptureArea(saved.Record.Region, extent)
			if err != nil || again != want {
				t.Fatalf("reopened area %v: %v", again, err)
			}
		})
	}
}

func TestWholeWindowCaptureMustBeResolvedBeforePersistence(t *testing.T) {
	request := WindowBindingRequest{Snapshot: "PIN-test", Region: desktop.WholeWindowCapture()}
	if err := request.Validate(); err != nil {
		t.Fatal(err)
	}
	session := &WindowSession{region: desktop.WholeWindowCapture(), confirm: func(context.Context, ClientWindow) error { return nil }}
	if _, err := CaptureWindowSession(context.Background(), t.TempDir(), "PIN-test", session); err == nil || err.Error() != "live.capture_extent_unresolved" {
		t.Fatal(err)
	}
}
