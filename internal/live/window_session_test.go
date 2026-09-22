package live

import (
	"context"
	"encoding/json"
	"errors"
	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
	"path/filepath"
	"strings"
	"testing"
)

type sessionFixtureFrames struct {
	ackFrames
	closed bool
}

func (f *sessionFixtureFrames) Close() { f.closed = true }

func TestWindowSessionObservation(t *testing.T) {
	ctx := context.Background()
	target := ClientWindow{Client: selection.ClientInstallation{Product: "retail", FullBuild: "12.1.0.69875"}, Window: desktop.WindowIdentity{Handle: 1, ProcessID: 2, ProcessStartedAt: 3}}
	expected := bridge.SignalExpectation{Kind: "ready", Release: "2.0.0-dev", SessionNonce: strings.Repeat("a", 32), Character: "Paladin", Realm: "Realm", Product: target.Client.Product, Build: target.Client.FullBuild, RequireInputReady: true}
	signal := bridge.Signal{Schema: "lycheedev.signal.v1", Kind: "ready", Release: expected.Release, SessionNonce: expected.SessionNonce, Character: expected.Character, Realm: expected.Realm, Product: expected.Product, Build: expected.Build, GUID: "Player-1-123", Sequence: 1, InputReady: true}
	for _, mode := range []string{"valid", "discovery", "discovery-invalid-nonce", "discovery-invalid-actor", "missing-guid", "wrong-nonce", "not-ready", "payload", "request", "changed-window", "invalid-expectation"} {
		t.Run(mode, func(t *testing.T) {
			candidate, want := signal, expected
			if strings.HasPrefix(mode, "discovery") {
				want.SessionNonce, want.Character, want.Realm = "", "", ""
			}
			switch mode {
			case "discovery-invalid-nonce":
				candidate.SessionNonce = "short"
			case "discovery-invalid-actor":
				candidate.Character = "bad\nname"
			case "missing-guid":
				candidate.GUID = ""
			case "wrong-nonce":
				candidate.SessionNonce = strings.Repeat("b", 32)
			case "not-ready":
				candidate.InputReady = false
			case "payload":
				candidate.ReportBytes = 2
				candidate.ReportAdler32 = "017500f9"
			case "request":
				candidate.RequestID = "OP-other"
			case "invalid-expectation":
				want.RequireInputReady = false
			}
			frames := &sessionFixtureFrames{ackFrames: ackFrames{frame: makeAckFrame(t, candidate)}}
			checks := 0
			confirm := func(context.Context, ClientWindow) error {
				checks++
				if mode == "changed-window" && checks == 2 {
					return desktop.ErrIdentityChanged
				}
				return nil
			}
			session, err := observeWindowSession(ctx, target, want, frames, confirm)
			if mode != "valid" && mode != "discovery" {
				if err == nil || session != nil || !frames.closed {
					t.Fatalf("accepted %s: %v", mode, err)
				}
				return
			}
			if err != nil || session.Ready() != signal || session.Target() != target || frames.closed || checks != 2 {
				t.Fatalf("valid: %v checks=%d", err, checks)
			}
			copy := session.Ready()
			copy.Character = "Other"
			if session.Ready().Character != "Paladin" {
				t.Fatal("caller changed session")
			}
			root := filepath.Join(t.TempDir(), "workspace")
			store, err := vault.Initialize(ctx, root)
			if err != nil {
				t.Fatal(err)
			}
			metadata, err := store.OpenMetadata(ctx)
			if err != nil {
				t.Fatal(err)
			}
			defer metadata.Close()
			pin, err := selection.OpenPinner(metadata).PinSelection(ctx, selection.SelectionSpec{Source: &selection.SourcePin{Repository: "fixture", Product: "retail", ExactCommit: strings.Repeat("a", 40), ParserRevision: "1"}})
			if err != nil {
				t.Fatal(err)
			}
			capture, err := CaptureWindowSession(ctx, root, pin.ID, session)
			if err != nil {
				t.Fatal(err)
			}
			ref, raw, err := evidence.OpenArchive(store, metadata).FetchCapture(ctx, capture.ID, 16384)
			if err != nil {
				t.Fatal(err)
			}
			var saved struct {
				Target ClientWindow
				Ready  bridge.Signal
			}
			if err := json.Unmarshal(raw, &saved); err != nil {
				t.Fatal(err)
			}
			if saved.Target != target || saved.Ready != signal || ref.Provenance.Kind != "decoded-window-session" || ref.Provenance.Snapshot != pin.ID {
				t.Fatal("session evidence changed")
			}
			record, err := SaveWindowSession(ctx, root, pin.ID, session)
			if err != nil {
				t.Fatal(err)
			}
			retained, err := ReadWindowSession(ctx, root, record.ID)
			if err != nil {
				t.Fatal(err)
			}
			if retained.Record != record || retained.Target != target || retained.Ready != signal {
				t.Fatal("session record changed")
			}
			bad := record
			bad.CaptureID = "CAP-missing"
			bad.ID = sessionRecordID(bad)
			if _, err := readSessionEvidence(ctx, store, metadata, bad); err == nil {
				t.Fatal("missing proof accepted")
			}
			other, err := selection.OpenPinner(metadata).PinSelection(ctx, selection.SelectionSpec{Source: &selection.SourcePin{Repository: "fixture", Product: "classic", ExactCommit: strings.Repeat("a", 40), ParserRevision: "1"}})
			if err != nil {
				t.Fatal(err)
			}
			if _, err := CaptureWindowSession(ctx, root, other.ID, session); err == nil {
				t.Fatal("foreign snapshot accepted")
			}
			bad = record
			bad.Snapshot = other.ID
			bad.ID = sessionRecordID(bad)
			if _, err := readSessionEvidence(ctx, store, metadata, bad); err == nil {
				t.Fatal("proof reattached to another snapshot")
			}
			session.Close()
			if !frames.closed {
				t.Fatal("capture leaked")
			}
			if _, err := CaptureWindowSession(ctx, root, pin.ID, session); err == nil {
				t.Fatal("closed session archived")
			}
			if _, err := SaveWindowSession(ctx, root, pin.ID, session); err == nil {
				t.Fatal("closed session saved")
			}
			if _, err := ReadWindowSession(ctx, root, record.ID); err != nil {
				t.Fatalf("closing live stream erased history: %v", err)
			}
			doc, err := metadata.ReadDocument(ctx, "session/"+record.ID)
			if err != nil {
				t.Fatal(err)
			}
			bad = record
			bad.CaptureID = "CAP-altered"
			changed, _ := json.Marshal(bad)
			if err := metadata.CommitDocuments(ctx, vault.Mutation{Key: doc.Key, ExpectedGeneration: doc.Generation, Value: changed}); err != nil {
				t.Fatal(err)
			}
			if _, err := ReadWindowSession(ctx, root, record.ID); err == nil {
				t.Fatal("tampered record accepted")
			}
		})
	}
	cancelled, cancel := context.WithCancel(ctx)
	cancel()
	frames := &sessionFixtureFrames{}
	if _, err := observeWindowSession(cancelled, target, expected, frames, func(ctx context.Context, _ ClientWindow) error { return ctx.Err() }); !errors.Is(err, context.Canceled) || !frames.closed {
		t.Fatalf("cancel: %v", err)
	}
}
