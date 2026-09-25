package live

import (
	"context"
	"errors"
	"path/filepath"

	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/vault"
)

// reconcileProbeReport needs neither a running client nor a capture stream.
// It retains the exact operation's writer lease and durable window ownership;
// verifying saved bytes never authorizes game input or retires the queue.
func reconcileProbeReport(ctx context.Context, root, id string) (record journal.WorkRecord, err error) {
	store, err := vault.OpenStore(root)
	if err != nil {
		return record, err
	}
	metadata, err := store.OpenMetadata(ctx)
	if err != nil {
		return record, err
	}
	defer func() { err = errors.Join(err, metadata.Close()) }()
	book := journal.OpenBook(metadata)
	record, err = book.InspectWork(ctx, id)
	if err != nil {
		return record, err
	}
	input, _, err := probeDefinition(record)
	if err != nil {
		return record, err
	}
	if input.Revision == "" {
		return record, journal.ErrTransition
	}
	run, err := book.AcquireWindowWork(ctx, filepath.Join(input.Load.Installation, "Interface", "AddOns"), store.Identity().WorkspaceID, id)
	if err != nil {
		return record, err
	}
	defer func() { err = errors.Join(err, run.Close()) }()
	for step := 0; step < 3; step++ {
		latest, checkErr := run.Check(ctx)
		if checkErr != nil {
			return record, checkErr
		}
		record = latest
		switch record.Stage {
		case "flush_requested":
			_, err = ObserveInstalledOperationPersisted(ctx, root, id)
		case "persisted":
			_, err = ArchiveInstalledOperationReport(ctx, root, id)
		case "verified":
			return record, nil
		default:
			return record, journal.ErrTransition
		}
		if err != nil {
			return record, err
		}
	}
	return record, journal.ErrTransition
}
