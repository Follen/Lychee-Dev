package live

import (
	"context"
	"errors"
	"io"
	"path/filepath"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"image"
)

// The dismissal input was submitted (or readiness could not be observed) but
// the window kept decoding bridge symbols inside the bounded verification
// window. Retryable; a busy window is a separate refusal.
var ErrReceiptHidePending = errors.New("live.receipt_hide_pending")

// Any in-flight operation owns this window's display; dismissal must not
// disturb its receipt even when the owner is this workspace.
var ErrReceiptWindowBusy = errors.New("live.receipt_window_busy")

const (
	receiptHideReadiness = 15 * time.Second
	receiptHideGrace     = 3 * time.Second
	receiptHideVerify    = 8 * time.Second
)

// HideReceiptResult is the read model of one explicit dismissal.
type HideReceiptResult struct {
	Session            string    `json:"session"`
	Cleared            bool      `json:"cleared"`
	MessagesQueued     int       `json:"messagesQueued"`
	SubmissionComplete bool      `json:"submissionComplete"`
	ObservedAt         time.Time `json:"observedAt"`
}

// hideIO carries the native seams so the dismissal flow stays testable
// against fixture frames and a fake sender.
type hideIO struct {
	send       preparedInput
	capture    func(context.Context, desktop.WindowIdentity, image.Rectangle) (sessionFrames, error)
	readiness  time.Duration
	grace      time.Duration
	verify     time.Duration
}

// HideReceipt dismisses the displayed bridge receipt on the session's window
// after the host has archived its evidence. It reconnects through the fixed
// bootstrap commands, refuses any window owned by an in-flight operation,
// observes fresh input readiness, submits exactly /dev bridge hide, and
// verifies the clear from valid frames.
func HideReceipt(ctx context.Context, root, sessionID string) (HideReceiptResult, error) {
	ctx, cancel := context.WithTimeout(ctx, 90*time.Second)
	defer cancel()
	session, _, err := reconnectSession(ctx, root, sessionID, nativeIO())
	if err != nil {
		return HideReceiptResult{}, err
	}
	defer session.Close()
	return hideReceipt(ctx, session, sessionID, hideIO{
		send:      desktop.QueuePreparedCommand,
		capture:   func(ctx context.Context, window desktop.WindowIdentity, region image.Rectangle) (sessionFrames, error) { return desktop.CaptureFrames(ctx, window, region) },
		readiness: receiptHideReadiness,
		grace:     receiptHideGrace,
		verify:    receiptHideVerify,
	})
}

func hideReceipt(ctx context.Context, session *WindowSession, sessionID string, native hideIO) (HideReceiptResult, error) {
	result := HideReceiptResult{Session: sessionID}
	parent := filepath.Join(session.target.Client.Directory, "Interface", "AddOns")
	if _, occupied, err := journal.InspectWindowOwner(ctx, parent, windowResource(session.target)); err != nil {
		return result, err
	} else if occupied {
		return result, ErrReceiptWindowBusy
	}
	// The input guard demands a freshly observed signal. A reconnected reader
	// has one observation, but the readiness wait also proves the displayed
	// receipt still matches this session before it is dismissed.
	expected := bridge.SignalExpectation{Kind: "ready", Release: session.ready.Release, SessionNonce: session.ready.SessionNonce,
		Character: session.ready.Character, Realm: session.ready.Realm, Product: session.ready.Product, Build: session.ready.Build,
		RuntimeEpoch: session.ready.RuntimeEpoch, RequireInputReady: true, AfterSequence: session.ready.Sequence - 1}
	wait, cancel := context.WithTimeout(ctx, native.readiness)
	defer cancel()
	if _, err := session.reader.WaitForSignal(wait, expected); err != nil {
		if ctx.Err() == nil && (errors.Is(err, context.DeadlineExceeded) || errors.Is(err, io.EOF)) {
			return result, errors.Join(ErrReceiptHidePending, err)
		}
		return result, err
	}
	sent := time.Now()
	receipt, err := native.send(ctx, session.target.Window,
		func(context.Context) (string, error) { return "/dev bridge hide", nil },
		func(ctx context.Context) error {
			if err := session.confirm(ctx, session.target); err != nil {
				return err
			}
			return session.reader.RequireFreshSignal()
		})
	result.MessagesQueued, result.SubmissionComplete = receipt.MessagesQueued, receipt.SubmissionComplete
	if err != nil {
		return result, err
	}
	cleared, observed := verifyReceiptCleared(ctx, native, session, sent)
	result.Cleared, result.ObservedAt = cleared, observed
	if !cleared {
		return result, ErrReceiptHidePending
	}
	return result, nil
}

// verifyReceiptCleared reads a dedicated stream: success requires consecutive
// valid, non-black frames decoding zero bridge symbols. A still-decoded
// receipt after the grace period keeps the failure honest instead of guessing.
func verifyReceiptCleared(ctx context.Context, native hideIO, session *WindowSession, sent time.Time) (bool, time.Time) {
	frames, err := native.capture(ctx, session.target.Window, session.region)
	if err != nil {
		return false, time.Time{}
	}
	defer frames.Close()
	deadline := time.Now().Add(native.verify)
	consecutive := 0
	for time.Now().Before(deadline) {
		frame, err := frames.Next(ctx)
		if err != nil || frame == nil || frame.NRGBA == nil {
			break
		}
		symbols, err := desktop.DecodeSymbols(frame.NRGBA)
		if err != nil {
			consecutive = 0
			continue
		}
		if len(symbols) > 0 {
			consecutive = 0
			if time.Since(sent) > native.grace {
				return false, frame.ObservedAt
			}
			continue
		}
		if frameHint(frame) == captureFrame {
			consecutive++
			if consecutive >= 2 {
				return true, frame.ObservedAt
			}
		}
	}
	return false, time.Time{}
}
