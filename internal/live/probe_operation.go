package live

import (
	"context"
	"encoding/json"
	"errors"
	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/vault"
	"path/filepath"
)

// ProbeOperation retains the runtime lease across phases. It borrows its live
// session; Close releases the lease/database but neither the capture stream nor
// durable ownership. Do not use it concurrently or after closing the session.
type ProbeOperation struct {
	session  *WindowSession
	root, id string
	run      *journal.WindowRun
	metadata *vault.Metadata
}

func (s *WindowSession) OpenOperation(ctx context.Context, root, operationID string) (result *ProbeOperation, err error) {
	store, err := vault.OpenStore(root)
	if err != nil {
		return nil, err
	}
	metadata, err := store.OpenMetadata(ctx)
	if err != nil {
		return nil, err
	}
	operation := &ProbeOperation{session: s, root: store.Root(), id: operationID, metadata: metadata}
	success := false
	defer func() {
		if !success {
			err = errors.Join(err, operation.Close())
		}
	}()
	book := journal.OpenBook(metadata)
	record, err := book.InspectWork(ctx, operationID)
	if err != nil {
		return nil, err
	}
	if _, err := s.operationInput(ctx, store.Root(), record); err != nil {
		return nil, err
	}
	if record.Intent.Resource != windowResource(s.target) {
		return nil, errors.New("live.operation_window_resource_mismatch")
	}
	if err := s.confirm(ctx, s.target); err != nil {
		return nil, err
	}
	operation.run, err = book.AcquireWindowWork(ctx, filepath.Join(s.target.Client.Directory, "Interface", "AddOns"), store.Identity().WorkspaceID, operationID)
	if err != nil {
		return nil, err
	}
	if err := operation.check(ctx); err != nil {
		return nil, err
	}
	success = true
	return operation, nil
}

func (p *ProbeOperation) Close() error {
	if p == nil {
		return nil
	}
	var err error
	if p.run != nil {
		err = p.run.Close()
		p.run = nil
	}
	if p.metadata != nil {
		err = errors.Join(err, p.metadata.Close())
		p.metadata = nil
	}
	return err
}

func (p *ProbeOperation) check(ctx context.Context) error {
	if p == nil || p.run == nil || p.metadata == nil {
		return errors.New("live.operation_closed")
	}
	record, err := p.run.Check(ctx)
	if err != nil {
		return err
	}
	if _, err := p.session.operationInput(ctx, p.root, record); err != nil {
		return err
	}
	if err := p.session.confirm(ctx, p.session.target); err != nil {
		return err
	}
	_, err = p.run.Check(ctx)
	return err
}

// Observe advances only loaded/reported/acknowledged receipt phases. It does not
// send input, persist SavedVariables, or interpret lease ownership as readiness.
func (p *ProbeOperation) Observe(ctx context.Context) (evidence.CaptureRef, error) {
	if err := p.check(ctx); err != nil {
		return evidence.CaptureRef{}, err
	}
	return p.session.observeOperation(ctx, p.root, p.id, p.check)
}

// PrepareFiles advances file-only work to the next game-observation boundary.
// It retains the runtime lease and checks the live identity between effects.
// Repeating it may reconcile queue publication, but never sends or replays input.
// On failure, inspect durable work: earlier file effects are not rolled back.
func (p *ProbeOperation) PrepareFiles(ctx context.Context) (journal.WorkRecord, error) {
	for step := 0; step < 4; step++ {
		if err := p.check(ctx); err != nil {
			return journal.WorkRecord{}, err
		}
		record, err := journal.OpenBook(p.metadata).InspectWork(ctx, p.id)
		if err != nil {
			return journal.WorkRecord{}, err
		}
		stop := false
		input, err := reportInput(record)
		if err != nil {
			return record, err
		}
		if input.Revision == "" {
			switch record.Stage {
			case "flush_requested", "persisted", "verified", "ack_requested":
				var observed reportObservation
				if err := json.Unmarshal(record.Observation, &observed); err != nil {
					return record, err
				}
				if observed.ReloadedCapture == "" {
					return record, errors.New("live.reload_not_observed")
				}
			}
		}
		switch record.Stage {
		case "prepared", "load_requested":
			_, err = PrepareOperationQueue(ctx, p.root, p.id)
			stop = true
		case "flush_requested":
			_, err = ObserveInstalledOperationPersisted(ctx, p.root, p.id)
		case "persisted":
			_, err = ArchiveInstalledOperationReport(ctx, p.root, p.id)
		case "verified":
			stop = true
		case "ack_requested":
			stop = true
		default:
			return journal.WorkRecord{}, journal.ErrTransition
		}
		if err != nil {
			return journal.WorkRecord{}, err
		}
		if stop {
			if err := p.check(ctx); err != nil {
				return journal.WorkRecord{}, err
			}
			return journal.OpenBook(p.metadata).InspectWork(ctx, p.id)
		}
	}
	return journal.WorkRecord{}, journal.ErrTransition
}
