package live

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"image/png"

	"github.com/follenfang/lycheedev/internal/bridge"

	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/vault"
)

type displayCompletion struct {
	Schema       string            `json:"schema"`
	Operation    string            `json:"operationId"`
	Binding      string            `json:"binding"`
	Receipt      HideReceiptResult `json:"receipt"`
	Checkpoint   string            `json:"checkpoint,omitempty"`
	Challenge    string            `json:"challenge,omitempty"`
	Frame        string            `json:"frame,omitempty"`
	RuntimeEpoch uint64            `json:"runtimeEpoch,omitempty"`
}

func displayProvenance(record journal.WorkRecord, binding string) evidence.Provenance {
	return evidence.Provenance{Kind: "decoded-display-clear", Locator: binding, OperationID: record.OperationID, Snapshot: record.Intent.Snapshot, Session: record.Intent.Session}
}

func readDisplayCompletion(ctx context.Context, root string, record journal.WorkRecord, binding string) (DisplayOutcome, bool, error) {
	result, err := vault.ReadWorkspace(ctx, root, func(store *vault.Store, metadata *vault.Metadata) (DisplayOutcome, error) {
		return readDisplayProof(ctx, store, metadata, record, binding)
	})
	return result, result.State == "cleared", err
}

func readDisplayProof(ctx context.Context, store *vault.Store, metadata *vault.Metadata, record journal.WorkRecord, binding string) (DisplayOutcome, error) {
	doc, err := metadata.ReadDocument(ctx, "display/"+record.OperationID)
	if errors.Is(err, vault.ErrMissingRecord) {
		return DisplayOutcome{}, nil
	}
	if err != nil {
		return DisplayOutcome{}, err
	}
	var link struct {
		Capture string `json:"capture"`
	}
	if json.Unmarshal(doc.Value, &link) != nil || link.Capture == "" {
		return DisplayOutcome{}, errors.New("live.invalid_display_completion")
	}
	ref, data, err := evidence.OpenArchive(store, metadata).FetchCapture(ctx, link.Capture, 4096)
	if err != nil {
		return DisplayOutcome{}, err
	}
	var proof displayCompletion
	if json.Unmarshal(data, &proof) != nil || proof.Schema != "lycheedev.display-completion.v1" || proof.Operation != record.OperationID || proof.Binding != binding || proof.Receipt.Session != binding || !proof.Receipt.Cleared || proof.Receipt.ObservedAt.IsZero() || !ref.Complete || ref.Truncated || ref.MediaType != "application/json" || ref.Provenance != displayProvenance(record, binding) {
		return DisplayOutcome{}, errors.New("live.invalid_display_completion")
	}
	if record.Intent.Goal == "finished" {
		if err := verifyFinishedDisplay(ctx, store, metadata, record, proof); err != nil {
			return DisplayOutcome{}, err
		}
	}
	return DisplayOutcome{State: "cleared", Capture: ref.ID, Receipt: &proof.Receipt}, nil
}

func saveDisplayCompletion(ctx context.Context, root string, record journal.WorkRecord, binding string, receipt HideReceiptResult) (DisplayOutcome, error) {
	if receipt.Session != binding || !receipt.Cleared || receipt.ObservedAt.IsZero() {
		return DisplayOutcome{}, errors.New("live.invalid_display_completion")
	}
	return saveDisplayProof(ctx, root, record, displayCompletion{Schema: "lycheedev.display-completion.v1", Operation: record.OperationID, Binding: binding, Receipt: receipt})
}

func saveDisplayProof(ctx context.Context, root string, record journal.WorkRecord, proof displayCompletion) (DisplayOutcome, error) {
	return vault.WriteMetadata(ctx, root, func(store *vault.Store, metadata *vault.Metadata) (DisplayOutcome, error) {
		if record.Intent.Goal == "finished" {
			if err := verifyFinishedDisplay(ctx, store, metadata, record, proof); err != nil {
				return DisplayOutcome{}, err
			}
		}
		data, _ := json.Marshal(proof)
		ref, err := evidence.OpenArchive(store, metadata).CommitCapture(ctx, evidence.CaptureDraft{Reader: bytes.NewReader(data), MaxBytes: 4096, MediaType: "application/json", Complete: true, Provenance: displayProvenance(record, proof.Binding)})
		if err != nil {
			return DisplayOutcome{}, err
		}
		raw, _ := json.Marshal(struct {
			Capture string `json:"capture"`
		}{ref.ID})
		if err := metadata.CommitDocuments(ctx, vault.Mutation{Key: "display/" + record.OperationID, Value: raw}); err != nil {
			return DisplayOutcome{}, err
		}
		return DisplayOutcome{State: "cleared", Capture: ref.ID, Receipt: &proof.Receipt}, nil
	})
}

func verifyFinishedDisplay(ctx context.Context, store *vault.Store, metadata *vault.Metadata, record journal.WorkRecord, proof displayCompletion) error {
	input, err := reportInput(record)
	if err != nil {
		return err
	}
	archive := evidence.OpenArchive(store, metadata)
	ref, raw, err := archive.FetchCapture(ctx, proof.Checkpoint, 4096)
	if err != nil {
		return err
	}
	if !ref.Complete || ref.Truncated || ref.MediaType != "application/json" || ref.Provenance != checkpointProvenance(record, input, proof.Challenge) {
		return errors.New("live.finalization_provenance_mismatch")
	}
	signal, err := bridge.ParseSignal(raw)
	if err != nil {
		return err
	}
	expected := input.Expected
	expected.Kind, expected.ProbeNonce, expected.AfterSequence, expected.RuntimeEpoch = "checkpoint", proof.Challenge, 0, proof.RuntimeEpoch
	if len(proof.Challenge) != 32 || proof.RuntimeEpoch == 0 || signal.WorkState != "finished" || !signal.ResourcesReleased {
		return errors.New("live.finalization_incomplete")
	}
	if err := validateCheckpoint(signal, input, expected); err != nil {
		return err
	}
	var observed reportObservation
	if err := json.Unmarshal(record.Observation, &observed); err != nil {
		return err
	}
	report, err := acknowledgedReport(ctx, archive, record, input, observed)
	if err != nil {
		return err
	}
	ack, err := bridge.ParseSignal([]byte(signal.Receipt))
	if err != nil {
		return err
	}
	ack = bridge.FillSignalIdentity(ack, bridge.SignalIdentity{Release: input.Expected.Release, Product: input.Expected.Product, Build: input.Expected.Build,
		Character: input.Expected.Character, Realm: input.Expected.Realm, GUID: input.Load.GUID, SessionNonce: input.Expected.SessionNonce})
	expected = input.Expected
	expected.Kind, expected.AfterSequence = "acknowledged", report.Receipt.Sequence
	if err := ack.Match(expected); err != nil {
		return err
	}
	if ack.CodeBytes != report.Receipt.CodeBytes || ack.CodeAdler32 != report.Receipt.CodeAdler32 || ack.ReportBytes != report.Receipt.ReportBytes || ack.ReportAdler32 != report.Receipt.ReportAdler32 {
		return errors.New("live.finalization_report_mismatch")
	}
	frame, pixels, err := archive.FetchCapture(ctx, proof.Frame, 32<<20)
	if err != nil {
		return err
	}
	want := evidence.Provenance{Kind: "wgc-finished-display", Locator: proof.Checkpoint, OperationID: record.OperationID, Snapshot: record.Intent.Snapshot, Session: record.Intent.Session}
	if !frame.Complete || frame.Truncated || frame.MediaType != "image/png" || frame.Provenance != want {
		return errors.New("live.finalization_frame_mismatch")
	}
	if _, err := png.DecodeConfig(bytes.NewReader(pixels)); err != nil {
		return err
	}
	return nil
}
