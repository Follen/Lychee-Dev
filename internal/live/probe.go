package live

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/delivery"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/vault"
	"hash/adler32"
	"path/filepath"
)

// ProbeLoadIntent is frozen with the report intent, not reconstructed from a
// queue file or receipt. It identifies the installation and expected reload.
type ProbeLoadIntent struct {
	Installation string `json:"installation"`
	Account      string `json:"account,omitempty"`
	GUID         string `json:"guid"`
	ReloadNonce  string `json:"reloadNonce"`
}
type probeLoadObservation struct {
	bootstrapObservation
	Schema            string                 `json:"schema"`
	Revision          delivery.QueueRevision `json:"revision"`
	LoadedCapture     string                 `json:"loadedCapture,omitempty"`
	LoadReadyCapture  string                 `json:"loadReadyCapture,omitempty"`
	LoadInput         *desktop.InputReceipt  `json:"loadInput,omitempty"`
	ReportedCapture   string                 `json:"reportedCapture,omitempty"`
	DispatchInput     *desktop.InputReceipt  `json:"dispatchInput,omitempty"`
	FlushInput        *desktop.InputReceipt  `json:"flushInput,omitempty"`
	FlushReadyCapture string                 `json:"flushReadyCapture,omitempty"`
	ReloadedCapture   string                 `json:"reloadedCapture,omitempty"`
}

func loadedOperationEvidence(ctx context.Context, archive *evidence.Archive, record journal.WorkRecord, input ReportIntent, definition bridge.ProbeDefinition) (probeLoadObservation, bridge.Signal, error) {
	var observed probeLoadObservation
	var signal bridge.Signal
	if err := json.Unmarshal(record.Observation, &observed); err != nil {
		return observed, signal, err
	}
	if observed.Schema != "lycheedev.probe-load.v1" || observed.Revision.SHA256 == "" || observed.LoadedCapture == "" {
		return observed, signal, errors.New("live.loaded_evidence_missing")
	}
	ref, data, err := archive.FetchCapture(ctx, observed.LoadedCapture, 4096)
	if err != nil {
		return observed, signal, err
	}
	want := evidence.Provenance{Kind: "decoded-game-loaded", Locator: definition.RequestID, Snapshot: record.Intent.Snapshot, OperationID: record.OperationID, DataBuild: definition.Build, Session: definition.SessionNonce}
	if ref.Provenance != want || !ref.Complete || ref.Truncated || ref.MediaType != "application/json" {
		return observed, signal, errors.New("live.loaded_evidence_mismatch")
	}
	signal, err = bridge.ParseSignal(data)
	if err != nil {
		return observed, signal, err
	}
	expected := input.Expected
	expected.Kind, expected.ReloadNonce, expected.RequireInputReady = "loaded", definition.ReloadNonce, false
	if err := signal.Match(expected); err != nil {
		return observed, signal, err
	}
	if signal.CodeBytes != uint32(len(input.Code)) || signal.CodeAdler32 != fmt.Sprintf("%08x", adler32.Checksum(input.Code)) {
		return observed, signal, errors.New("live.loaded_code_mismatch")
	}
	return observed, signal, nil
}

// RequestOperationDispatch persists intent before returning the one-shot slash
// command. It does not send input. The sender still must hold the selected window
// lease and recheck current identity/readiness; archived readiness is not a live
// permission. Interrupted dispatch_requested work must reconcile, never replay.
func RequestOperationDispatch(ctx context.Context, root, operationID string) (string, error) {
	return withOperation(ctx, root, operationID, func(store *vault.Store, metadata *vault.Metadata) (string, error) {
		book := journal.OpenBook(metadata)
		record, err := book.InspectWork(ctx, operationID)
		if err != nil {
			return "", err
		}
		if record.Stage != "loaded" || record.Status != "running" {
			return "", journal.ErrTransition
		}
		input, definition, err := probeDefinition(record)
		if err != nil {
			return "", err
		}
		_, signal, err := loadedOperationEvidence(ctx, evidence.OpenArchive(store, metadata), record, input, definition)
		if err != nil {
			return "", err
		}
		if !signal.InputReady {
			return "", errors.New("bridge.input_not_ready")
		}
		deployment, _, err := delivery.ResolveAddonDestination(ctx, input.Load.Installation)
		if err != nil {
			return "", err
		}
		if deployment.Client.Product != definition.Product || deployment.Client.FullBuild != definition.Build {
			return "", errors.New("live.probe_installation_mismatch")
		}
		if err := book.AdvanceStage(ctx, journal.StageChange{OperationID: record.OperationID, ExpectedGeneration: record.Generation, ExpectedStage: record.Stage, Stage: "dispatch_requested", Status: "running", Observation: record.Observation}); err != nil {
			return "", err
		}
		return "/dev bridge run " + definition.RequestID, nil
	})
}

// ObserveOperationReported records a fresh receipt, not a verified report body
// or persisted output. The same selected-window reader must span all phases.
func ObserveOperationReported(ctx context.Context, root, operationID string, reader *bridge.SignalReader) (evidence.CaptureRef, error) {
	if reader == nil {
		return evidence.CaptureRef{}, errors.New("live.missing_signal_reader")
	}
	return observeOperationReported(ctx, root, operationID, reader.WaitForSignal)
}

func observeOperationReported(ctx context.Context, root, operationID string, wait signalWait) (evidence.CaptureRef, error) {
	return withOperation(ctx, root, operationID, func(store *vault.Store, metadata *vault.Metadata) (evidence.CaptureRef, error) {
		var zero evidence.CaptureRef
		book := journal.OpenBook(metadata)
		record, err := book.InspectWork(ctx, operationID)
		if err != nil {
			return zero, err
		}
		if record.Stage != "dispatch_requested" || (record.Status != "running" && record.Status != "unresolved") {
			return zero, journal.ErrTransition
		}
		input, definition, err := probeDefinition(record)
		if err != nil {
			return zero, err
		}
		archive := evidence.OpenArchive(store, metadata)
		observed, loaded, err := loadedOperationEvidence(ctx, archive, record, input, definition)
		if err != nil {
			return zero, err
		}
		expected := input.Expected
		expected.Kind, expected.ReloadNonce, expected.AfterSequence, expected.RequireInputReady = "reported", "", loaded.Sequence, false
		signal, err := wait(ctx, expected)
		if err != nil {
			return zero, err
		}
		if signal.ReloadNonce != "" || signal.CodeBytes != loaded.CodeBytes || signal.CodeAdler32 != loaded.CodeAdler32 {
			return zero, errors.New("live.reported_code_mismatch")
		}
		payload, err := json.Marshal(signal)
		if err != nil {
			return zero, err
		}
		capture, err := archive.CommitCapture(ctx, evidence.CaptureDraft{Reader: bytes.NewReader(payload), MaxBytes: 4096, MediaType: "application/json", Complete: true, Provenance: evidence.Provenance{Kind: "decoded-game-reported", Locator: definition.RequestID, Snapshot: record.Intent.Snapshot, OperationID: record.OperationID, DataBuild: definition.Build, Session: definition.SessionNonce}})
		if err != nil {
			return zero, err
		}
		observed.ReportedCapture = capture.ID
		raw, _ := json.Marshal(observed)
		err = book.AdvanceStage(ctx, journal.StageChange{OperationID: record.OperationID, ExpectedGeneration: record.Generation, ExpectedStage: record.Stage, Stage: "reported", Status: "running", Observation: raw})
		return capture, err
	})
}

func probeDefinition(record journal.WorkRecord) (ReportIntent, bridge.ProbeDefinition, error) {
	input, err := reportInput(record)
	if err != nil {
		return input, bridge.ProbeDefinition{}, err
	}
	if record.Intent.Kind != "probe" || input.Load == nil || !filepath.IsAbs(input.Load.Installation) {
		return input, bridge.ProbeDefinition{}, errors.New("live.invalid_probe_load_intent")
	}
	expected := input.Expected
	definition := bridge.ProbeDefinition{RequestID: expected.RequestID, Release: expected.Release, SessionNonce: expected.SessionNonce, ReloadNonce: input.Load.ReloadNonce, Character: expected.Character, Realm: expected.Realm, GUID: input.Load.GUID, Product: expected.Product, Build: expected.Build, Code: string(input.Code)}
	_, err = bridge.EncodeProbeQueue([]bridge.ProbeDefinition{definition})
	return input, definition, err
}

// PrepareOperationQueue records load intent before publishing definitions. A
// resumed load_requested stage may repeat this idempotent merge; it never sends
// reload/dispatch input, even if queue preparation was previously successful.
func PrepareOperationQueue(ctx context.Context, root, operationID string) (delivery.QueueRevision, error) {
	return withOperation(ctx, root, operationID, func(_ *vault.Store, metadata *vault.Metadata) (delivery.QueueRevision, error) {
		var zero delivery.QueueRevision
		book := journal.OpenBook(metadata)
		record, err := book.InspectWork(ctx, operationID)
		if err != nil {
			return zero, err
		}
		if (record.Stage != "prepared" && record.Stage != "load_requested") || (record.Status != "pending" && record.Status != "running" && record.Status != "unresolved") {
			return zero, journal.ErrTransition
		}
		input, definition, err := probeDefinition(record)
		if err != nil {
			return zero, err
		}
		deployment, parent, err := delivery.ResolveAddonDestination(ctx, input.Load.Installation)
		if err != nil {
			return zero, err
		}
		if deployment.Client.Product != definition.Product || deployment.Client.FullBuild != definition.Build {
			return zero, errors.New("live.probe_installation_mismatch")
		}
		observation := probeLoadObservation{Schema: "lycheedev.probe-load.v1"}
		raw, _ := json.Marshal(observation)
		if record.Stage == "prepared" {
			if err := book.AdvanceStage(ctx, journal.StageChange{OperationID: record.OperationID, ExpectedGeneration: record.Generation, ExpectedStage: record.Stage, Stage: "load_requested", Status: "running", Observation: raw}); err != nil {
				return zero, err
			}
			record, err = book.InspectWork(ctx, operationID)
			if err != nil {
				return zero, err
			}
		} else {
			if err := json.Unmarshal(record.Observation, &observation); err != nil {
				return zero, err
			}
			if observation.Schema != "lycheedev.probe-load.v1" {
				return zero, errors.New("live.invalid_probe_load_observation")
			}
		}
		revision, err := delivery.ChangeProbeQueue(ctx, filepath.Join(parent, "Lychee Dev"), definition, false)
		if err != nil {
			return zero, err
		}
		// Changed describes this invocation, not the stored queue identity.
		// A reconciled retry must not churn the durable work generation.
		if !revision.Changed && observation.Revision.SHA256 == revision.SHA256 && observation.Revision.Entries == revision.Entries {
			return revision, nil
		}
		observation.Revision = revision
		raw, _ = json.Marshal(observation)
		err = book.AdvanceStage(ctx, journal.StageChange{OperationID: record.OperationID, ExpectedGeneration: record.Generation, ExpectedStage: record.Stage, Stage: "load_requested", Status: "running", Observation: raw})
		// Failed observation persistence leaves the earlier durable intent. Report
		// the file revision as partial progress, never imply a loaded game state.
		return revision, err
	})
}

// ObserveOperationLoaded requires a fresh matching signal and verifies the
// actual code checksum before archiving evidence and advancing to loaded.
// Callers retain the selected window's reader across phases; a new reader must
// not be created here to bypass its frame watermark.
func ObserveOperationLoaded(ctx context.Context, root, operationID string, reader *bridge.SignalReader) (evidence.CaptureRef, error) {
	if reader == nil {
		return evidence.CaptureRef{}, errors.New("live.missing_signal_reader")
	}
	return observeOperationLoaded(ctx, root, operationID, reader.WaitForSignal)
}

func observeOperationLoaded(ctx context.Context, root, operationID string, wait signalWait) (evidence.CaptureRef, error) {
	return withOperation(ctx, root, operationID, func(store *vault.Store, metadata *vault.Metadata) (evidence.CaptureRef, error) {
		var zero evidence.CaptureRef
		book := journal.OpenBook(metadata)
		record, err := book.InspectWork(ctx, operationID)
		if err != nil {
			return zero, err
		}
		if (record.Stage != "load_requested" && record.Stage != "loaded") || (record.Status != "running" && record.Status != "unresolved") {
			return zero, journal.ErrTransition
		}
		input, definition, err := probeDefinition(record)
		if err != nil {
			return zero, err
		}
		var observation probeLoadObservation
		if err := json.Unmarshal(record.Observation, &observation); err != nil {
			return zero, err
		}
		if observation.Schema != "lycheedev.probe-load.v1" || observation.Revision.SHA256 == "" {
			return zero, errors.New("live.probe_queue_not_prepared")
		}
		if observation.BootstrapReadyID != "" && observation.BootstrapCapture == "" {
			return zero, errors.New("live.bootstrap_not_observed")
		}
		expected := input.Expected
		expected.Kind, expected.ReloadNonce, expected.RequireInputReady = "loaded", definition.ReloadNonce, false
		var previous *bridge.Signal
		if record.Stage == "loaded" {
			_, archived, err := loadedOperationEvidence(ctx, evidence.OpenArchive(store, metadata), record, input, definition)
			if err != nil {
				return zero, err
			}
			// A new frame may still display the exact final loaded receipt.
			// Recovery needs fresh pixels, not an unsolicited new addon event.
			previous = &archived
			expected.AfterSequence = archived.Sequence - 1
		}
		signal, err := wait(ctx, expected)
		if err != nil {
			return zero, err
		}
		if previous != nil && signal.Sequence == previous.Sequence && signal != *previous {
			return zero, errors.New("live.loaded_sequence_reused")
		}
		if previous != nil && signal.Sequence == previous.Sequence && !signal.InputReady {
			return zero, errors.New("bridge.input_not_ready")
		}
		if signal.CodeBytes != uint32(len(input.Code)) || signal.CodeAdler32 != fmt.Sprintf("%08x", adler32.Checksum(input.Code)) {
			return zero, errors.New("live.loaded_code_mismatch")
		}
		payload, err := json.Marshal(signal)
		if err != nil {
			return zero, err
		}
		capture, err := evidence.OpenArchive(store, metadata).CommitCapture(ctx, evidence.CaptureDraft{Reader: bytes.NewReader(payload), MaxBytes: 4096, MediaType: "application/json", Complete: true, Provenance: evidence.Provenance{Kind: "decoded-game-loaded", Locator: definition.RequestID, Snapshot: record.Intent.Snapshot, OperationID: record.OperationID, DataBuild: definition.Build, Session: definition.SessionNonce}})
		if err != nil {
			return zero, err
		}
		observation.LoadedCapture = capture.ID
		raw, _ := json.Marshal(observation)
		err = book.AdvanceStage(ctx, journal.StageChange{OperationID: record.OperationID, ExpectedGeneration: record.Generation, ExpectedStage: record.Stage, Stage: "loaded", Status: "running", Observation: raw})
		return capture, err
	})
}
