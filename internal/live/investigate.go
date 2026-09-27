package live

import (
	"context"
	"crypto/sha256"
	"errors"
	"fmt"
	"time"

	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/vault"
)

// ExecuteRequest is a complete investigation of one saved target. Code and
// Probe are alternatives; file bytes are frozen directly in the operation.
type ExecuteRequest struct {
	LoadProbeRequest
	Code []byte
}

func (r ExecuteRequest) Validate() error {
	if (r.Probe == "") == (len(r.Code) == 0) {
		return errors.New("live.execute_requires_one_source")
	}
	if len(r.Code) > 256<<10 {
		return errors.New("bridge.queue_invalid_code")
	}
	base := r.LoadProbeRequest
	if len(r.Code) > 0 {
		base.Probe = "inline"
	}
	return base.Validate()
}

// Execute owns load, execution, report verification, ACK and display cleanup.
// Repeating its request resumes the original operation before any reconnect.
func Execute(ctx context.Context, root string, request ExecuteRequest) (Outcome, error) {
	if err := request.Validate(); err != nil {
		return Outcome{}, err
	}
	var revision ProbeRevision
	if request.Probe != "" {
		probe, err := ShowProbe(ctx, root, request.Probe)
		if err != nil {
			return Outcome{}, err
		}
		revision = probe.Revision
	} else {
		digest := fmt.Sprintf("%x", sha256.Sum256(request.Code))
		revision = ProbeRevision{Schema: "lycheedev.probe-revision.v1", ID: probeRevisionPrefix + digest,
			SHA256: digest, Bytes: len(request.Code), Code: append([]byte(nil), request.Code...), CreatedAt: time.Now().UTC()}
	}
	record, found, err := lookupProbeRequest(ctx, root, request.LoadProbeRequest, revision, "finished")
	if err != nil {
		return Outcome{}, err
	}
	if found {
		return Resume(ctx, root, record.OperationID)
	}
	// Readiness is refreshed only when admitting new work. Its target is the
	// exact saved process/window/actor/build, never discovery's first candidate.
	connect, cancel := context.WithTimeout(ctx, loadPhaseBudget)
	session, snapshot, err := reconnectSession(connect, root, request.Session, nativeIO())
	if err != nil {
		cancel()
		return Outcome{}, err
	}
	if session.ready.ReportScope != "character-v1" {
		session.Close()
		cancel()
		return Outcome{}, errors.New("live.investigation_protocol_required")
	}
	session.writerIO = nativeIO()
	record, err = session.prepareProbe(connect, root, snapshot, request.Account, request.Request, revision.ID,
		"finished", revision.Code, request.BudgetSeconds)
	session.Close()
	cancel()
	if err != nil {
		return finishOutcome(ctx, root, record, err)
	}
	return Resume(ctx, root, record.OperationID)
}

func lookupProbeRequest(ctx context.Context, root string, request LoadProbeRequest, revision ProbeRevision, completion string) (journal.WorkRecord, bool, error) {
	bound, err := ReadWindowSession(ctx, root, request.Session)
	if err != nil {
		return journal.WorkRecord{}, false, err
	}
	type lookup struct {
		Record journal.WorkRecord
		Found  bool
	}
	result, err := vault.ReadWorkspace(ctx, root, func(_ *vault.Store, metadata *vault.Metadata) (lookup, error) {
		book := journal.OpenBook(metadata)
		identity := func(account string) journal.WorkIntent {
			return probeRequestIdentity(bound.Target, bound.Ready, bound.Record.Snapshot, account,
				request.Request, revision.ID, completion, revision.Code, request.BudgetSeconds)
		}
		prior, found, err := book.FindRequest(ctx, identity("").RequestKey)
		if err != nil || !found {
			return lookup{}, err
		}
		input, err := reportInput(prior)
		if err != nil {
			return lookup{}, err
		}
		account := request.Account
		if account == "" {
			account = input.Load.Account
		}
		record, found, err := book.LookupRequest(ctx, identity(account))
		return lookup{record, found}, err
	})
	return result.Record, result.Found, err
}
