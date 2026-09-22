package live

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/json"
	"errors"
	"fmt"
	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
	"image"
)

type SessionRecord struct {
	Schema    string          `json:"schema"`
	ID        string          `json:"id"`
	Snapshot  string          `json:"snapshot"`
	CaptureID string          `json:"captureId"`
	Region    image.Rectangle `json:"region"`
}

type RecordedSession struct {
	Record SessionRecord `json:"record"`
	Target ClientWindow  `json:"target"`
	Ready  bridge.Signal `json:"ready"`
}

// Connection is the user-facing view of retained identity, not another state
// record. Protocol fields stay in the verifiable capture, not ordinary output.
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

// SaveWindowSession publishes only after the decoded window evidence is durable.
// An interruption may leave a discoverable capture, never an evidence-free record.
func SaveWindowSession(ctx context.Context, root, snapshot string, session *WindowSession) (SessionRecord, error) {
	capture, err := CaptureWindowSession(ctx, root, snapshot, session)
	if err != nil {
		return SessionRecord{}, err
	}
	return vault.WriteMetadata(ctx, root, func(store *vault.Store, metadata *vault.Metadata) (SessionRecord, error) {
		record := SessionRecord{Schema: "lycheedev.session.v1", Snapshot: snapshot, CaptureID: capture.ID, Region: session.region}
		record.ID = sessionRecordID(record)
		if _, err := readSessionEvidence(ctx, store, metadata, record); err != nil {
			return SessionRecord{}, err
		}
		data, err := json.Marshal(record)
		if err != nil {
			return SessionRecord{}, err
		}
		if err := metadata.CommitDocuments(ctx, vault.Mutation{Key: "session/" + record.ID, Value: data}); err != nil {
			return SessionRecord{}, err
		}
		return record, nil
	})
}

// ReadWindowSession verifies retained evidence only. It does not attach a live
// stream, refresh readiness, or authorize resuming input in the old process.
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
