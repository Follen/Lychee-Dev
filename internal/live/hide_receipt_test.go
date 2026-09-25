package live

import (
	"context"
	"errors"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"image"
	"image/color"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/vault"
)

// repeatFrames yields one frame forever; it models a live capture stream.
type repeatFrames struct {
	frame  *desktop.CapturedFrame
	closed bool
}

func (f *repeatFrames) Close() { f.closed = true }
func (f *repeatFrames) Next(ctx context.Context) (*desktop.CapturedFrame, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if f.frame == nil {
		return nil, os.ErrClosed
	}
	return f.frame, nil
}

func blankFrame(t *testing.T, at time.Time) *desktop.CapturedFrame {
	t.Helper()
	pixels := image.NewNRGBA(image.Rect(0, 0, 48, 48))
	for y := pixels.Bounds().Min.Y; y < pixels.Bounds().Max.Y; y++ {
		for x := pixels.Bounds().Min.X; x < pixels.Bounds().Max.X; x++ {
			pixels.SetNRGBA(x, y, color.NRGBA{R: 200, G: 200, B: 200, A: 255})
		}
	}
	return &desktop.CapturedFrame{NRGBA: pixels, ObservedAt: at}
}

func hideSendRecorder(t *testing.T, commands *[]string) preparedInput {
	t.Helper()
	return func(ctx context.Context, _ desktop.WindowIdentity, prepare func(context.Context) (string, error), guard func(context.Context) error) (desktop.InputReceipt, error) {
		if err := guard(ctx); err != nil {
			return desktop.InputReceipt{}, err
		}
		command, err := prepare(ctx)
		if err != nil {
			return desktop.InputReceipt{}, err
		}
		*commands = append(*commands, command)
		if err := guard(ctx); err != nil {
			return desktop.InputReceipt{}, err
		}
		return desktop.InputReceipt{MessagesQueued: 3, SubmissionComplete: true}, nil
	}
}

// releaseFixtureOwner retires the fixture probe so its window is unowned:
// the journal transition clears workspace ownership, RetireWindowWork removes
// the on-disk window marker.
func releaseFixtureOwner(t *testing.T, root, client string, book *journal.Book, record journal.WorkRecord) {
	t.Helper()
	if err := book.AdvanceStage(context.Background(), journal.StageChange{OperationID: record.OperationID, ExpectedGeneration: record.Generation, ExpectedStage: record.Stage, Stage: "cleaned", Status: "cancelled", Observation: record.Observation}); err != nil {
		t.Fatal(err)
	}
	var workspaceID string
	if _, err := vault.ReadWorkspace(context.Background(), root, func(store *vault.Store, _ *vault.Metadata) (struct{}, error) {
		workspaceID = store.Identity().WorkspaceID
		return struct{}{}, nil
	}); err != nil {
		t.Fatal(err)
	}
	if err := book.RetireWindowWork(context.Background(), filepath.Join(client, "Interface", "AddOns"), workspaceID, record.OperationID); err != nil {
		t.Fatal(err)
	}
}

func TestHideReceiptClearsDisplayedReceipt(t *testing.T) {
	ctx := context.Background()
	root, client, book, record, input := probeOperationFixtureAtSequence(t, 7)
	releaseFixtureOwner(t, root, client, book, record)
	session, frames := operationSessionFixtureAtSequence(t, client, input, 7)
	// The reconnect observation consumed the fixture frame; re-arm the feed
	// with the still-displayed ready receipt the readiness wait must match.
	ready := session.ready
	displayed := ready
	rearmed := makeAckFrame(t, displayed)
	rearmed.SystemTicks = 2 // the discovery observation already consumed tick 1
	frames.ackFrames.frame = rearmed
	var commands []string
	result, err := hideReceipt(ctx, session, "saved", hideIO{
		send:      hideSendRecorder(t, &commands),
		capture:   func(context.Context, desktop.WindowIdentity, image.Rectangle) (sessionFrames, error) { return &repeatFrames{frame: blankFrame(t, time.Now())}, nil },
		readiness: 5 * time.Second,
		grace:     time.Second,
		verify:    2 * time.Second,
	})
	if err != nil {
		t.Fatalf("hide: %+v %v", result, err)
	}
	if !result.Cleared || !result.SubmissionComplete || result.MessagesQueued != 3 {
		t.Fatalf("hide result: %+v", result)
	}
	if len(commands) != 1 || commands[0] != "/dev bridge hide" {
		t.Fatalf("hide commands: %#v", commands)
	}
	if result.ObservedAt.IsZero() {
		t.Fatal("clearance lacked an observation timestamp")
	}
	_ = root
}

func TestHideReceiptRefusesOwnedWindow(t *testing.T) {
	ctx := context.Background()
	_, client, _, _, input := probeOperationFixtureAtSequence(t, 7)
	session, _ := operationSessionFixtureAtSequence(t, client, input, 7)
	// Occupy exactly this window with an in-flight operation marker.
	owner := journal.WindowOwner{Schema: "lycheedev.window-owner.v1", WorkspaceID: strings.Repeat("a", 32), Resource: windowResource(session.target), OperationID: "OP-" + strings.Repeat("b", 32)}
	raw, err := json.Marshal(owner)
	if err != nil {
		t.Fatal(err)
	}
	markers := filepath.Join(client, "Interface", "AddOns", ".lycheedev-window-owners")
	if err := os.MkdirAll(markers, 0o700); err != nil {
		t.Fatal(err)
	}
	digest := sha256.Sum256([]byte(windowResource(session.target)))
	if err := os.WriteFile(filepath.Join(markers, hex.EncodeToString(digest[:])+".json"), raw, 0o600); err != nil {
		t.Fatal(err)
	}
	var commands []string
	_, err = hideReceipt(ctx, session, "saved", hideIO{
		send:      hideSendRecorder(t, &commands),
		capture:   func(context.Context, desktop.WindowIdentity, image.Rectangle) (sessionFrames, error) { t.Fatal("captured on refusal"); return nil, nil },
		readiness: time.Second, grace: time.Second, verify: time.Second,
	})
	if !errors.Is(err, ErrReceiptWindowBusy) || len(commands) != 0 {
		t.Fatalf("owned window not refused: %v commands=%v", err, commands)
	}
}

func TestHideReceiptPendingWhenStillVisible(t *testing.T) {
	ctx := context.Background()
	root, client, book, record, input := probeOperationFixtureAtSequence(t, 7)
	releaseFixtureOwner(t, root, client, book, record)
	session, frames := operationSessionFixtureAtSequence(t, client, input, 7)
	displayed := session.ready
	rearmed := makeAckFrame(t, displayed)
	rearmed.SystemTicks = 2 // the discovery observation already consumed tick 1
	frames.ackFrames.frame = rearmed
	var commands []string
	result, err := hideReceipt(ctx, session, "saved", hideIO{
		send: hideSendRecorder(t, &commands),
		capture: func(context.Context, desktop.WindowIdentity, image.Rectangle) (sessionFrames, error) {
			return &repeatFrames{frame: makeAckFrame(t, displayed)}, nil
		},
		readiness: 5 * time.Second,
		grace:     50 * time.Millisecond,
		verify:    2 * time.Second,
	})
	if err == nil || result.Cleared || !errors.Is(err, ErrReceiptHidePending) {
		t.Fatalf("still-visible receipt wrong outcome: %+v %v", result, err)
	}
	if result.MessagesQueued == 0 || !result.SubmissionComplete {
		t.Fatalf("submission receipt lost: %+v %v", result, err)
	}
}

func TestHideReceiptReadinessPending(t *testing.T) {
	ctx := context.Background()
	root, client, book, record, input := probeOperationFixtureAtSequence(t, 7)
	releaseFixtureOwner(t, root, client, book, record)
	session, frames := operationSessionFixtureAtSequence(t, client, input, 7)
	// The reconnect consumed the frame; an empty feed leaves no observation.
	frames.ackFrames.frame = nil
	var commands []string
	result, err := hideReceipt(ctx, session, "saved", hideIO{
		send: hideSendRecorder(t, &commands),
		capture: func(context.Context, desktop.WindowIdentity, image.Rectangle) (sessionFrames, error) {
			t.Fatal("captured before readiness")
			return nil, nil
		},
		readiness: 2 * time.Second, grace: time.Second, verify: time.Second,
	})
	if err == nil || len(commands) != 0 {
		t.Fatalf("missing readiness not pending: %v commands=%v", err, commands)
	}
	if result.SubmissionComplete {
		t.Fatal("readiness failure must not submit")
	}
}

func TestHideReceiptMatchesSessionIdentity(t *testing.T) {
	ctx := context.Background()
	root, client, book, record, input := probeOperationFixtureAtSequence(t, 7)
	releaseFixtureOwner(t, root, client, book, record)
	session, frames := operationSessionFixtureAtSequence(t, client, input, 7)
	foreign := session.ready
	foreign.SessionNonce = strings.Repeat("f", 32)
	rearmed := makeAckFrame(t, foreign)
	rearmed.SystemTicks = 2
	frames.ackFrames.frame = rearmed
	var commands []string
	_, err := hideReceipt(ctx, session, "saved", hideIO{
		send: hideSendRecorder(t, &commands),
		capture: func(context.Context, desktop.WindowIdentity, image.Rectangle) (sessionFrames, error) {
			return &repeatFrames{frame: blankFrame(t, time.Now())}, nil
		},
		readiness: 300 * time.Millisecond, grace: time.Second, verify: time.Second,
	})
	if err == nil || len(commands) != 0 {
		t.Fatalf("foreign receipt dismissed: %v commands=%v", err, commands)
	}
}
