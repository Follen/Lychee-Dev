// Historical records only: no game capture, input, or execution adapters.
package live

import (
	"bytes"
	"context"
	"crypto/sha256"
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
	"unicode/utf8"
)

type SessionRecord struct {
	Schema    string                   `json:"schema"`
	ID        string                   `json:"id"`
	Snapshot  string                   `json:"snapshot"`
	CaptureID string                   `json:"captureId"`
	Region    image.Rectangle          `json:"region"`
	Bindings  desktop.ReceiverBindings `json:"bindings,omitempty"`
}

type RecordedSession struct {
	Record SessionRecord `json:"record"`
	Target ClientWindow  `json:"target"`
	Ready  bridge.Signal `json:"ready"`
}

// Connection presents historical identity without turning an old ready receipt
// or binding into current input authority.
type Connection struct {
	ID        string       `json:"id"`
	Snapshot  string       `json:"snapshot"`
	Target    ClientWindow `json:"target"`
	Character string       `json:"character"`
	Realm     string       `json:"realm"`
	CaptureID string       `json:"captureId"`
}

func (s RecordedSession) Connection() Connection {
	return Connection{ID: s.Record.ID, Snapshot: s.Record.Snapshot, Target: s.Target, Character: s.Ready.Character, Realm: s.Ready.Realm, CaptureID: s.Record.CaptureID}
}

type windowSessionEvidence struct {
	Schema string        `json:"schema"`
	Target ClientWindow  `json:"target"`
	Ready  bridge.Signal `json:"ready"`
}

func sessionRecordID(record SessionRecord) string {
	record.ID = ""
	data, _ := json.Marshal(record)
	return fmt.Sprintf("SESSION-%x", sha256.Sum256(data))
}

func ReadWindowSession(ctx context.Context, root, id string) (RecordedSession, error) {
	return vault.ReadWorkspace(ctx, root, func(store *vault.Store, metadata *vault.Metadata) (RecordedSession, error) {
		doc, err := metadata.ReadDocument(ctx, "session/"+id)
		if err != nil {
			return RecordedSession{}, err
		}
		var record SessionRecord
		if err := json.Unmarshal(doc.Value, &record); err != nil {
			return RecordedSession{}, err
		}
		if record.ID != id {
			return RecordedSession{}, errors.New("live.session_record_identity")
		}
		return readSessionEvidence(ctx, store, metadata, record)
	})
}

func readSessionEvidence(ctx context.Context, store *vault.Store, metadata *vault.Metadata, record SessionRecord) (RecordedSession, error) {
	var zero RecordedSession
	if record.Schema != "lycheedev.session.v1" || record.ID != sessionRecordID(record) {
		return zero, errors.New("live.session_record_integrity")
	}
	if record.Bindings.WakeBinding == "" {
		record.Bindings = desktop.DefaultReceiverBindings()
	}
	if err := desktop.ValidateReceiverBindings(record.Bindings); err != nil {
		return zero, err
	}
	pin, err := selection.OpenPinner(metadata).ReadPinnedSet(ctx, record.Snapshot)
	if err != nil {
		return zero, err
	}
	ref, data, err := evidence.OpenArchive(store, metadata).FetchCapture(ctx, record.CaptureID, 16384)
	if err != nil {
		return zero, err
	}
	var payload windowSessionEvidence
	if err := json.Unmarshal(data, &payload); err != nil {
		return zero, err
	}
	canonical, err := json.Marshal(payload)
	if err != nil || !bytes.Equal(canonical, data) || payload.Schema != "lycheedev.window-session.v1" {
		return zero, errors.New("live.session_evidence_invalid")
	}
	signal := payload.Ready
	raw, _ := json.Marshal(signal)
	if _, err := bridge.ParseSignal(raw); err != nil {
		return zero, err
	}
	expected := bridge.SignalExpectation{Kind: "ready", Release: signal.Release, SessionNonce: signal.SessionNonce, Character: signal.Character, Realm: signal.Realm, Product: signal.Product, Build: signal.Build, RequireInputReady: true}
	if err := sessionExpectation(payload.Target, expected); err != nil {
		return zero, err
	}
	if err := signal.Match(expected); err != nil {
		return zero, err
	}
	if signal.GUID == "" || signal.RequestID != "" || signal.ReloadNonce != "" || signal.CodeBytes != 0 || signal.CodeAdler32 != "" || signal.ReportBytes != 0 || signal.ReportAdler32 != "" || signal.Sequence > 9007199254740991 {
		return zero, errors.New("live.invalid_session_signal")
	}
	if err := sessionSnapshotMatches(pin, payload.Target); err != nil {
		return zero, err
	}
	want := evidence.Provenance{Kind: "decoded-window-session", Locator: fmt.Sprintf("pid:%d/window:%d", payload.Target.Window.ProcessID, payload.Target.Window.Handle), Snapshot: record.Snapshot, DataBuild: signal.Build, Session: signal.SessionNonce}
	if ref.Provenance != want || !ref.Complete || ref.Truncated || ref.MediaType != "application/json" {
		return zero, errors.New("live.session_evidence_provenance")
	}
	return RecordedSession{Record: record, Target: payload.Target, Ready: signal}, nil
}

func sessionSnapshotMatches(pin selection.PinnedSet, target ClientWindow) error {
	product, build := target.Client.Product, target.Client.FullBuild
	if pin.Source != nil && pin.Source.Product != product || pin.Data != nil && (pin.Data.Product != product || pin.Data.FullBuild != build) || pin.Changes != nil && (pin.Changes.Product != product || pin.Changes.FullBuild != build) {
		return errors.New("live.session_snapshot_mismatch")
	}
	return nil
}

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

func bindingLabel(label string) bool {
	if len(label) > 128 || !utf8.ValidString(label) {
		return false
	}
	for _, c := range label {
		if c < 32 || c == 127 {
			return false
		}
	}
	return true
}
