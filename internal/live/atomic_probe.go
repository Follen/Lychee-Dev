package live

import (
	"context"
	"errors"
	"strings"
	"time"

	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/vault"
)

type LoadProbeRequest struct {
	Session string
	Account string
	Probe   string
	Request string
}

func (r LoadProbeRequest) Validate() error {
	if strings.TrimSpace(r.Session) == "" || len(r.Session) > 128 {
		return errors.New("live.session_required")
	}
	if strings.TrimSpace(r.Probe) == "" || len(r.Probe) > 128 {
		return errors.New("live.probe_selector_invalid")
	}
	if strings.TrimSpace(r.Request) == "" || len(r.Request) > 128 {
		return errors.New("live.request_key_invalid")
	}
	if r.Account != "" {
		return validateReportAccount(r.Account)
	}
	return nil
}

// LoadProbe publishes one immutable source revision and performs only the
// correlated reload/load handshake. It never dispatches the source.
func LoadProbe(ctx context.Context, root string, request LoadProbeRequest) (Outcome, error) {
	var record journal.WorkRecord
	if err := request.Validate(); err != nil {
		return Outcome{}, err
	}
	revision, err := ShowProbe(ctx, root, request.Probe)
	if err != nil {
		return Outcome{}, err
	}
	ctx, cancel := context.WithTimeout(ctx, 2*time.Minute)
	defer cancel()
	session, snapshot, err := reconnectSession(ctx, root, request.Session, nativeIO())
	if err != nil {
		return Outcome{}, err
	}
	defer session.Close()
	record, err = session.PrepareProbeRevision(ctx, root, snapshot, request.Account, request.Request, revision.Revision)
	if err != nil {
		return finishOutcome(ctx, root, record, err)
	}
	if record.Stage == "cleaned" && (record.Status == "completed" || record.Status == "cancelled") {
		return finishOutcome(ctx, root, record, nil)
	}
	operation, err := session.OpenOperation(ctx, root, record.OperationID)
	if err != nil {
		return finishOutcome(ctx, root, record, err)
	}
	defer operation.Close()
	record, err = operation.execute(ctx, desktop.QueuePreparedCommand)
	return finishOutcome(ctx, root, record, err)
}

// RunLoaded advances exactly one loaded operation to a verified local report.
func RunLoaded(ctx context.Context, root, id string) (Outcome, error) {
	record, err := setOperationGoal(ctx, root, id, "loaded", "verified")
	if err != nil {
		return finishOutcome(ctx, root, record, err)
	}
	if record.Stage == "verified" || record.Stage == "cleaned" {
		return finishOutcome(ctx, root, record, nil)
	}
	return Resume(ctx, root, id)
}

// AcknowledgeVerified advances exactly one verified operation through its ACK
// receipt and exact queue retirement. It does not force another game reload.
func AcknowledgeVerified(ctx context.Context, root, id string) (Outcome, error) {
	record, err := setOperationGoal(ctx, root, id, "verified", "cleaned")
	if err != nil {
		return finishOutcome(ctx, root, record, err)
	}
	if record.Stage == "cleaned" {
		return finishOutcome(ctx, root, record, nil)
	}
	return Resume(ctx, root, id)
}

func setOperationGoal(ctx context.Context, root, id, stage, goal string) (journal.WorkRecord, error) {
	return vault.WriteMetadata(ctx, root, func(_ *vault.Store, metadata *vault.Metadata) (journal.WorkRecord, error) {
		return journal.OpenBook(metadata).SetGoal(ctx, id, stage, goal)
	})
}
