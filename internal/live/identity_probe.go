package live

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"errors"
	"fmt"
	"image"
	"path/filepath"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/buildinfo"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/records"
	"github.com/follenfang/lycheedev/internal/selection"
)

// liveIO is the native seam behind discovery and connection. It is replaced
// wholesale in tests; callers of the deep modules never see these steps.
type liveIO struct {
	list    func(context.Context) ([]desktop.WindowIdentity, error)
	inspect func(context.Context, string) (selection.ClientInstallation, error)
	capture func(context.Context, desktop.WindowIdentity, image.Rectangle) (sessionFrames, error)
	send    func(context.Context, desktop.WindowIdentity, string) (desktop.InputReceipt, error)
	confirm func(context.Context, ClientWindow) error
	peek    func(context.Context, string, string) (journal.WindowOwner, bool, error)
	wait    time.Duration
	refresh time.Duration
}

func nativeIO() *liveIO {
	return &liveIO{
		list:    desktop.ListWindows,
		inspect: records.InspectClientInstallation,
		capture: func(ctx context.Context, window desktop.WindowIdentity, region image.Rectangle) (sessionFrames, error) {
			return desktop.CaptureFrames(ctx, window, region)
		},
		send:    desktop.QueueBootstrapCommand,
		confirm: ConfirmClientWindow,
		peek:    journal.InspectWindowOwner,
		wait:    10 * time.Second,
		refresh: 8 * time.Second,
	}
}

const inputKeyboardFocus = "input_keyboard_focus"

// Identity marker states of one running window. They are observations about
// the world, never permissions.
const (
	CandidateIdentified      = "identified"
	CandidateBusy            = "busy"
	CandidateNoActor         = "no_actor"
	CandidateActorRestricted = "actor_restricted"
	CandidateUnreadable      = "identity_unreadable"
)

const (
	captureNoFrame    = "no_frame"
	captureFrame      = "frame"
	captureBlackFrame = "black_frame"
)

// identityObservation is one nonce-correlated identity receipt plus a cheap
// capture hint for unreadable windows.
type identityObservation struct {
	Signal     bridge.Signal
	Capture    string
	ObservedAt time.Time
}

type captureHints struct {
	feed       sessionFrames
	frames     int
	hint       string
	observedAt time.Time
}

func (h *captureHints) Next(ctx context.Context) (*desktop.CapturedFrame, error) {
	frame, err := h.feed.Next(ctx)
	if err != nil {
		return nil, err
	}
	h.frames++
	h.hint = frameHint(frame)
	if frame != nil {
		h.observedAt = frame.ObservedAt
	}
	return frame, nil
}

func (h *captureHints) observation(signal bridge.Signal) identityObservation {
	hint := captureNoFrame
	if h.frames > 0 {
		hint = h.hint
	}
	return identityObservation{Signal: signal, Capture: hint, ObservedAt: h.observedAt}
}

// frameHint samples a fixed 5x5 grid; it is a hint for humans, not evidence.
func frameHint(frame *desktop.CapturedFrame) string {
	if frame == nil || frame.NRGBA == nil {
		return captureFrame
	}
	bounds := frame.Bounds()
	if bounds.Dx() < 5 || bounds.Dy() < 5 {
		return captureFrame
	}
	dark := 0
	for y := 0; y < 5; y++ {
		for x := 0; x < 5; x++ {
			px := bounds.Min.X + x*(bounds.Dx()-1)/4
			py := bounds.Min.Y + y*(bounds.Dy()-1)/4
			r, g, b, _ := frame.At(px, py).RGBA()
			if r>>8 < 16 && g>>8 < 16 && b>>8 < 16 {
				dark++
			}
		}
	}
	if dark == 25 {
		return captureBlackFrame
	}
	return captureFrame
}

// probeIdentity sends exactly one fixed identity trigger to one window and
// accepts only the receipt correlated by its fresh probe nonce. Windows are
// probed one at a time: the caller serializes keystrokes across windows. With
// gate set it also tolerates the standard focus-delayed redisplay so callers
// can gate on a single input-ready observation.
func probeIdentity(ctx context.Context, target ClientWindow, region image.Rectangle, gate bool, io *liveIO) (identityObservation, error) {
	var zero identityObservation
	entropy := make([]byte, 16)
	if _, err := rand.Read(entropy); err != nil {
		return zero, err
	}
	nonce := hex.EncodeToString(entropy)
	frames, err := io.capture(ctx, target.Window, region)
	if err != nil {
		return zero, err
	}
	defer frames.Close()
	hints := &captureHints{feed: frames}
	if _, err := io.send(ctx, target.Window, "/dev bridge identify "+nonce); err != nil {
		return hints.observation(bridge.Signal{}), err
	}
	reader := bridge.ObserveSignals(hints)
	expected := bridge.SignalExpectation{Kind: "identity", Release: buildinfo.Version, ProbeNonce: nonce, Product: target.Client.Product, Build: target.Client.FullBuild}
	wait, cancel := context.WithTimeout(ctx, io.wait)
	defer cancel()
	signal, err := reader.DiscoverIdentity(wait, expected)
	var mismatch *bridge.RuntimeReleaseMismatch
	if err != nil && !errors.As(err, &mismatch) {
		return hints.observation(bridge.Signal{}), err
	}
	observation := hints.observation(signal)
	if gate && !signal.InputReady && signal.InputReason == inputKeyboardFocus {
		// The typed command leaves keyboard focus in the chat edit box and the
		// addon re-displays a refreshed receipt once focus is released. Waiting
		// for that display is observation, not a second trigger or a guess.
		refreshed := expected
		refreshed.Release = signal.Release
		refreshed.RequireInputReady = true
		wait, cancel := context.WithTimeout(ctx, io.refresh)
		defer cancel()
		if next, err := reader.DiscoverIdentity(wait, refreshed); err == nil {
			observation = hints.observation(next)
		}
	}
	return observation, err
}

// windowOwnership reports whether an in-flight operation owns this window's
// input. Uncertain ownership never permits input.
func windowOwnership(ctx context.Context, workspaceID string, target ClientWindow, io *liveIO) (journal.WindowOwner, bool, bool, error) {
	parent := filepath.Join(target.Client.Directory, "Interface", "AddOns")
	owner, occupied, err := io.peek(ctx, parent, windowResource(target))
	if err != nil {
		return owner, true, true, err
	}
	if !occupied {
		return owner, false, false, nil
	}
	return owner, true, owner.WorkspaceID != workspaceID, nil
}

func notReadyReason(signal bridge.Signal) string {
	if signal.InputReason != "" {
		return signal.InputReason
	}
	if signal.ActorState == "no_actor" {
		return "input_not_logged_in"
	}
	return "input_not_ready"
}

func identityUnreadable(err error) error {
	return fmt.Errorf("%w: %v", ErrIdentityUnreadable, err)
}
