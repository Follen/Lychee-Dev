package live

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/delivery"
	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/vault"
	"path/filepath"
)

// RetireQueue removes only this operation's exact original definition after ACK.
// Disk deletion may still await cleanup reload; demanding it here would make
// retirement depend on the reload that must first unload this queue entry.
// It records intent before the file effect and can reconcile an interrupted
// removal. The operation remains acknowledged: this does not unload game code,
// send reload, mark cleaned, or release the durable window owner.
func (p *ProbeOperation) RetireQueue(ctx context.Context) (delivery.QueueRevision, error) {
	var zero delivery.QueueRevision
	if err := p.check(ctx); err != nil {
		return zero, err
	}
	return withOperation(ctx, p.root, p.id, func(store *vault.Store, metadata *vault.Metadata) (delivery.QueueRevision, error) {
		book := journal.OpenBook(metadata)
		record, err := book.InspectWork(ctx, p.id)
		if err != nil {
			return zero, err
		}
		if record.Stage != "acknowledged" || record.Status != "running" && record.Status != "unresolved" {
			return zero, journal.ErrTransition
		}
		input, definition, err := probeDefinition(record)
		if err != nil {
			return zero, err
		}
		var observed reportObservation
		if err := json.Unmarshal(record.Observation, &observed); err != nil {
			return zero, err
		}
		report, err := acknowledgedReport(ctx, evidence.OpenArchive(store, metadata), record, input, observed)
		if err != nil {
			return zero, err
		}
		state, err := readInstalledState(ctx, input.Load.Installation, input.Load.Account, input.Expected)
		if err != nil {
			return zero, err
		}
		if state.Path != observed.SourcePath || (observed.RemovalID != "" && state.FileSHA256 != observed.RemovalSHA256) {
			return zero, errors.New("live.removal_source_changed")
		}
		if observed.RemovalID == "" {
			persisted, presentErr := bridge.ReadPersistedReport(bytes.NewReader(state.Bytes), input.Code, input.Expected)
			if presentErr == nil {
				if !bytes.Equal(persisted.ReceiptBytes, report.ReceiptBytes) || !bytes.Equal(persisted.Body, report.Body) {
					return zero, errors.New("live.retirement_report_changed")
				}
			} else if _, err := bridge.VerifyArchivedReportRemoval(report.ReceiptBytes, report.Body, bytes.NewReader(state.Bytes), input.Code, input.Expected); err != nil {
				return zero, errors.Join(presentErr, err)
			}
		} else if _, err := bridge.VerifyArchivedReportRemoval(report.ReceiptBytes, report.Body, bytes.NewReader(state.Bytes), input.Code, input.Expected); err != nil {
			return zero, err
		}
		if err := p.check(ctx); err != nil {
			return zero, err
		}
		save := func() error {
			raw, _ := json.Marshal(observed)
			if err := book.AdvanceStage(ctx, journal.StageChange{OperationID: p.id, ExpectedGeneration: record.Generation, ExpectedStage: "acknowledged", Stage: "acknowledged", Status: "running", Observation: raw}); err != nil {
				return err
			}
			record, err = book.InspectWork(ctx, p.id)
			return err
		}
		if observed.QueueRetirement == nil {
			observed.QueueRetirement = &queueRetirement{}
			if err := save(); err != nil {
				return zero, err
			}
		}
		if err := p.check(ctx); err != nil {
			return zero, err
		}
		revision, err := delivery.ChangeProbeQueue(ctx, filepath.Join(p.session.target.Client.Directory, "Interface", "AddOns", "Lychee Dev"), definition, true)
		if err != nil {
			return zero, err
		}
		if err := p.check(ctx); err != nil {
			return revision, err
		}
		previous := observed.QueueRetirement
		if !revision.Changed && previous.Revision.SHA256 == revision.SHA256 && previous.Revision.Entries == revision.Entries {
			return revision, nil
		}
		observed.QueueRetirement = &queueRetirement{Revision: revision}
		return revision, save()
	})
}
