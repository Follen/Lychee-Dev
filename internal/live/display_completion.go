package live

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"

	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/vault"
)

type displayCompletion struct {
	Schema    string            `json:"schema"`
	Operation string            `json:"operationId"`
	Binding   string            `json:"binding"`
	Receipt   HideReceiptResult `json:"receipt"`
}

func displayProvenance(record journal.WorkRecord, binding string) evidence.Provenance {
	return evidence.Provenance{Kind: "decoded-display-clear", Locator: binding, OperationID: record.OperationID, Snapshot: record.Intent.Snapshot, Session: record.Intent.Session}
}

func readDisplayCompletion(ctx context.Context, root string, record journal.WorkRecord, binding string) (DisplayOutcome, bool, error) {
	result, err := vault.ReadWorkspace(ctx, root, func(store *vault.Store, metadata *vault.Metadata) (DisplayOutcome, error) {
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
		return DisplayOutcome{State: "cleared", Capture: ref.ID, Receipt: &proof.Receipt}, nil
	})
	return result, result.State == "cleared", err
}

func saveDisplayCompletion(ctx context.Context, root string, record journal.WorkRecord, binding string, receipt HideReceiptResult) (DisplayOutcome, error) {
	if receipt.Session != binding || !receipt.Cleared || receipt.ObservedAt.IsZero() {
		return DisplayOutcome{}, errors.New("live.invalid_display_completion")
	}
	return vault.WriteMetadata(ctx, root, func(store *vault.Store, metadata *vault.Metadata) (DisplayOutcome, error) {
		data, _ := json.Marshal(displayCompletion{Schema: "lycheedev.display-completion.v1", Operation: record.OperationID, Binding: binding, Receipt: receipt})
		ref, err := evidence.OpenArchive(store, metadata).CommitCapture(ctx, evidence.CaptureDraft{Reader: bytes.NewReader(data), MaxBytes: 4096, MediaType: "application/json", Complete: true, Provenance: displayProvenance(record, binding)})
		if err != nil {
			return DisplayOutcome{}, err
		}
		raw, _ := json.Marshal(struct {
			Capture string `json:"capture"`
		}{ref.ID})
		if err := metadata.CommitDocuments(ctx, vault.Mutation{Key: "display/" + record.OperationID, Value: raw}); err != nil {
			return DisplayOutcome{}, err
		}
		return DisplayOutcome{State: "cleared", Capture: ref.ID, Receipt: &receipt}, nil
	})
}
