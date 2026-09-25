package live

import (
	"context"
	"encoding/json"
	"errors"
	"path/filepath"

	"github.com/follenfang/lycheedev/internal/delivery"
	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/vault"
)

// FinalizeAcknowledged completes the explicit ACK command without manufacturing
// another reload. The host already owns verified report bytes; the exact queue
// entry is retired and the game acknowledgement is retained as evidence.
func (p *ProbeOperation) FinalizeAcknowledged(ctx context.Context) (journal.WorkRecord, error) {
	if _, err := p.RetireQueue(ctx); err != nil {
		return journal.WorkRecord{}, err
	}
	if err := p.check(ctx); err != nil {
		return journal.WorkRecord{}, err
	}
	record, err := journal.OpenBook(p.metadata).InspectWork(ctx, p.id)
	if err != nil {
		return record, err
	}
	if record.Stage != "acknowledged" || record.Status != "running" && record.Status != "unresolved" {
		return record, journal.ErrTransition
	}
	input, definition, err := probeDefinition(record)
	if err != nil {
		return record, err
	}
	var observed reportObservation
	if err := json.Unmarshal(record.Observation, &observed); err != nil {
		return record, err
	}
	store, err := vault.OpenStore(p.root)
	if err != nil {
		return record, err
	}
	if _, err := acknowledgedReport(ctx, evidence.OpenArchive(store, p.metadata), record, input, observed); err != nil {
		return record, err
	}
	if observed.QueueRetirement == nil || observed.QueueRetirement.Revision.SHA256 == "" {
		return record, errors.New("live.queue_retirement_unconfirmed")
	}
	if _, err := delivery.VerifyProbeRetired(ctx, delivery.AddonDirectory(p.session.target.Client.Directory), definition); err != nil {
		return record, err
	}
	book := journal.OpenBook(p.metadata)
	if err := book.AdvanceStage(ctx, journal.StageChange{OperationID: p.id, ExpectedGeneration: record.Generation, ExpectedStage: "acknowledged", Stage: "cleaned", Status: "completed", Observation: record.Observation}); err != nil {
		return record, err
	}
	completed, err := book.InspectWork(ctx, p.id)
	if err != nil {
		return completed, err
	}
	if err := p.run.Close(); err != nil {
		return completed, err
	}
	p.run = nil
	if err := book.RetireWindowWork(ctx, filepath.Join(p.session.target.Client.Directory, "Interface", "AddOns"), store.Identity().WorkspaceID, p.id); err != nil {
		return completed, err
	}
	return completed, nil
}
