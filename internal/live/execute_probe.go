package live

import (
	"context"
	"encoding/json"
	"errors"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"io"
	"time"
)

// Execute advances an owned operation through confirmed game and disk phases.
// It never resubmits durable input intent. On error it returns the latest record
// for recovery; Close remains the caller's responsibility. Reopening a session
// after process loss is separate from advancing this retained session.
func (p *ProbeOperation) Execute(ctx context.Context) (journal.WorkRecord, error) {
	return p.execute(ctx, desktop.QueuePreparedCommand)
}

func (p *ProbeOperation) execute(ctx context.Context, send preparedInput) (journal.WorkRecord, error) {
	var record journal.WorkRecord
	if p == nil || p.metadata == nil || p.run == nil {
		return record, errors.New("live.operation_closed")
	}
	if send == nil {
		return record, errors.New("live.input_sender_missing")
	}
	book := journal.OpenBook(p.metadata)
	ackRetried := false
	// Every successful step advances a phase or commits an evidence reference.
	// This bound detects accidental non-progress without a polling loop.
	for step := 0; step < 20; step++ {
		var err error
		record, err = book.InspectWork(ctx, p.id)
		if err != nil {
			return record, err
		}
		if err := ctx.Err(); err != nil {
			return record, err
		}
		if record.Status != "pending" && record.Status != "running" && record.Status != "unresolved" {
			return record, journal.ErrTransition
		}
		if operationGoalReached(record) {
			return record, nil
		}
		var observed struct {
			bootstrapObservation
			LoadReadyCapture string `json:"loadReadyCapture"`
			ReloadedCapture  string `json:"reloadedCapture"`
			CleanupReadyID   string `json:"cleanupReadyId"`
		}
		if len(record.Observation) != 0 {
			if err := json.Unmarshal(record.Observation, &observed); err != nil {
				return record, err
			}
		}
		switch record.Stage {
		case "prepared":
			_, err = p.bootstrap(ctx, send)
		case "load_requested":
			switch {
			case observed.BootstrapReadyID == "":
				_, err = p.bootstrap(ctx, send)
			case observed.BootstrapCapture == "":
				_, err = p.ObserveBootstrap(ctx)
			case observed.LoadReadyCapture == "":
				_, err = p.load(ctx, send)
			case record.Intent.Goal == "loaded":
				// Load-only work must archive readiness without dispatching. A
				// later run reacquires fresh loaded evidence under its input lock.
				_, err = p.Observe(ctx)
			default:
				// Dispatch observes loaded readiness while holding the input
				// mutex. Observing separately here could consume the final ready
				// sequence and then wait for an unnecessary second refresh.
				_, err = p.dispatch(ctx, send)
			}
		case "loaded":
			_, err = p.dispatch(ctx, send)
		case "dispatch_requested":
			_, err = p.Observe(ctx)
		case "ack_requested":
			input, inputErr := reportInput(record)
			if inputErr != nil {
				err = inputErr
				break
			}
			// Retained pre-atomic records keep their original observe-only
			// protocol. The atomic revision uses the idempotent ACK endpoint.
			canRetry := input.Revision != "" && !ackRetried
			if canRetry && unsentAckIntent(record) {
				ackRetried = true
				_, err = p.retryAcknowledgement(ctx, send)
				break
			}
			_, err = p.Observe(ctx)
			if canRetry && ctx.Err() == nil && (errors.Is(err, io.EOF) || errors.Is(err, context.DeadlineExceeded)) {
				// ACK is idempotent for the exact request and report sequence.
				// A retry still requires new input-ready pixels under the input
				// lock. Never resend the probe, and never treat missing pixels as ACK.
				ackRetried = true
				_, retryErr := p.retryAcknowledgement(ctx, send)
				if retryErr == nil {
					err = nil
				} else {
					err = errors.Join(err, retryErr)
				}
			}
		case "reported":
			_, err = p.flush(ctx, send)
		case "flush_requested":
			input, inputErr := reportInput(record)
			if inputErr != nil {
				err = inputErr
				break
			}
			// Retained pre-atomic operations include a correlated cleanup
			// reload and keep their original completion contract.
			if input.Revision == "" {
				if observed.ReloadedCapture == "" {
					_, err = p.ObserveReload(ctx)
				} else {
					_, err = p.PrepareFiles(ctx)
				}
				break
			}
			// The exact durable report proves the result independently of the
			// transient reentry display. Try it before waiting for pixels.
			prepared, prepareErr := p.PrepareFiles(ctx)
			err = prepareErr
			if err == nil {
				break
			}
			// Archival failures after persistence are not reload failures.
			if prepared.OperationID == "" {
				prepared, _ = InspectOperation(ctx, p.root, p.id)
			}
			if prepared.Stage != "flush_requested" {
				break
			}
			if observed.ReloadedCapture == "" {
				_, reloadErr := p.ObserveReload(ctx)
				// Saving may finish while the display is unavailable. One final
				// bounded file check, never replay reload or the probe.
				_, err = p.PrepareFiles(ctx)
				if err != nil {
					err = errors.Join(errors.New("live.report_persistence_unconfirmed"), err, reloadErr)
				}
			}
		case "persisted":
			_, err = p.PrepareFiles(ctx)
		case "verified":
			_, err = p.acknowledge(ctx, send)
		case "acknowledged":
			input, inputErr := reportInput(record)
			if inputErr != nil {
				err = inputErr
				break
			}
			if input.Revision != "" {
				var completed journal.WorkRecord
				completed, err = p.FinalizeAcknowledged(ctx)
				if err == nil {
					return completed, nil
				}
				break
			}
			if observed.CleanupReadyID != "" {
				var completed journal.WorkRecord
				completed, err = p.Complete(ctx)
				if err == nil {
					return completed, nil
				}
			} else {
				_, err = p.clean(ctx, send)
			}
		default:
			err = journal.ErrTransition
		}
		if err != nil {
			// Input cancellation may have committed an unresolved outcome. Read
			// it without reusing the cancelled context; this performs no input.
			inspect, cancel := context.WithTimeout(context.WithoutCancel(ctx), 2*time.Second)
			latest, inspectErr := book.InspectWork(inspect, p.id)
			cancel()
			if inspectErr == nil {
				record = latest
			}
			return record, errors.Join(err, inspectErr)
		}
	}
	return record, errors.New("live.execution_step_limit")
}

func operationGoalReached(record journal.WorkRecord) bool {
	switch record.Intent.Goal {
	case "loaded":
		return record.Stage == "loaded"
	case "verified":
		return record.Stage == "verified"
	case "cleaned", "": // Empty is the retained 2.0.1 work-record contract.
		return record.Stage == "cleaned" && (record.Status == "completed" || record.Status == "cancelled")
	default:
		return false
	}
}
