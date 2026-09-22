package live

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
	"image"
	"strings"
	"time"
)

type sessionFrames interface {
	bridge.FrameFeed
	Close()
}

// WindowSession owns one capture stream and its monotonic frame watermark.
// It is a live observation, not a persistent permission or an input sender.
// The caller must Close it and must not use it concurrently across operations.
type WindowSession struct {
	target  ClientWindow
	region  image.Rectangle
	ready   bridge.Signal
	reader  *bridge.SignalReader
	frames  sessionFrames
	confirm func(context.Context, ClientWindow) error
	closed  bool
}

func (s *WindowSession) Close() {
	if s != nil && !s.closed {
		s.closed = true
		if s.frames != nil {
			s.frames.Close()
		}
	}
}
func (s *WindowSession) Target() ClientWindow { return s.target }
func (s *WindowSession) Ready() bridge.Signal { return s.ready }

func sessionExpectation(target ClientWindow, expected bridge.SignalExpectation) error {
	if !bindingLabel(expected.Character) || !bindingLabel(expected.Realm) {
		return errors.New("live.invalid_binding_identity")
	}
	if expected.Kind != "ready" || expected.Release == "" || expected.RequestID != "" || expected.ReloadNonce != "" || expected.CleanupNonce != "" || expected.Product != target.Client.Product || expected.Build != target.Client.FullBuild || !expected.RequireInputReady {
		return errors.New("live.invalid_session_expectation")
	}
	if expected.SessionNonce != "" && (expected.Character == "" || expected.Realm == "" || len(expected.SessionNonce) != 32 || strings.Trim(expected.SessionNonce, "0123456789abcdef") != "") {
		return errors.New("live.invalid_session_expectation")
	}
	return nil
}

// OpenWindowSession captures only the already resolved native window. It cannot
// accept caller-supplied signal JSON or switch windows when matching fails.
// The player opts in with /dev connect. Initial discovery may omit actor and
// nonce filters; reconnecting always supplies the recorded identity. Neither
// path types into the game or changes which native window is being observed.
func OpenWindowSession(ctx context.Context, target ClientWindow, region image.Rectangle, expected bridge.SignalExpectation) (*WindowSession, error) {
	if err := sessionExpectation(target, expected); err != nil {
		return nil, err
	}
	if err := ConfirmClientWindow(ctx, target); err != nil {
		return nil, err
	}
	frames, err := desktop.CaptureFrames(ctx, target.Window, region)
	if err != nil {
		return nil, err
	}
	session, err := observeWindowSession(ctx, target, expected, frames, ConfirmClientWindow)
	if err == nil {
		session.region = region
	}
	return session, err
}

func observeWindowSession(ctx context.Context, target ClientWindow, expected bridge.SignalExpectation, frames sessionFrames, confirm func(context.Context, ClientWindow) error) (*WindowSession, error) {
	success := false
	defer func() {
		if !success {
			frames.Close()
		}
	}()
	if err := sessionExpectation(target, expected); err != nil {
		return nil, err
	}
	if err := confirm(ctx, target); err != nil {
		return nil, err
	}
	reader := bridge.ObserveSignals(frames)
	wait, cancel := context.WithTimeout(ctx, 15*time.Second)
	defer cancel()
	var signal bridge.Signal
	var err error
	if expected.SessionNonce == "" {
		signal, err = reader.DiscoverReady(wait, expected)
	} else {
		signal, err = reader.WaitForSignal(wait, expected)
	}
	if err != nil {
		return nil, err
	}
	if signal.GUID == "" || signal.RequestID != "" || signal.ReloadNonce != "" || signal.CodeBytes != 0 || signal.CodeAdler32 != "" || signal.ReportBytes != 0 || signal.ReportAdler32 != "" || signal.Sequence > 9007199254740991 {
		return nil, errors.New("live.invalid_session_signal")
	}
	identity := expected
	identity.SessionNonce, identity.Character, identity.Realm = signal.SessionNonce, signal.Character, signal.Realm
	if err := sessionExpectation(target, identity); err != nil {
		return nil, err
	}
	if err := confirm(ctx, target); err != nil {
		return nil, err
	}
	success = true
	return &WindowSession{target: target, ready: signal, reader: reader, frames: frames, confirm: confirm}, nil
}

// CaptureWindowSession retains normalized decoded evidence, not a screenshot
// or permission that survives process/session changes. A durable live binding
// may reference this capture, but must still revalidate before every effect.
func CaptureWindowSession(ctx context.Context, root, snapshot string, session *WindowSession) (evidence.CaptureRef, error) {
	if session == nil || session.closed || session.confirm == nil {
		return evidence.CaptureRef{}, errors.New("live.session_closed")
	}
	return vault.WriteMetadata(ctx, root, func(store *vault.Store, metadata *vault.Metadata) (evidence.CaptureRef, error) {
		var zero evidence.CaptureRef
		pin, err := selection.OpenPinner(metadata).ReadPinnedSet(ctx, snapshot)
		if err != nil {
			return zero, err
		}
		build := session.target.Client.FullBuild
		if err := sessionSnapshotMatches(pin, session.target); err != nil {
			return zero, err
		}
		if err := session.confirm(ctx, session.target); err != nil {
			return zero, err
		}
		payload, err := json.Marshal(windowSessionEvidence{"lycheedev.window-session.v1", session.target, session.ready})
		if err != nil {
			return zero, err
		}
		return evidence.OpenArchive(store, metadata).CommitCapture(ctx, evidence.CaptureDraft{Reader: bytes.NewReader(payload), MaxBytes: 16384, MediaType: "application/json", Complete: true, Provenance: evidence.Provenance{Kind: "decoded-window-session", Locator: fmt.Sprintf("pid:%d/window:%d", session.target.Window.ProcessID, session.target.Window.Handle), Snapshot: pin.ID, DataBuild: build, Session: session.ready.SessionNonce}})
	})
}
