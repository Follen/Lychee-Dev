package live

import (
	"context"
	"encoding/json"
	"errors"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/live/journal"
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
			default:
				// Dispatch observes loaded readiness while holding the input
				// mutex. Observing separately here could consume the final ready
				// sequence and then wait for an unnecessary second refresh.
				_, err = p.dispatch(ctx, send)
			}
		case "loaded":
			_, err = p.dispatch(ctx, send)
		case "dispatch_requested", "ack_requested":
			_, err = p.Observe(ctx)
		case "reported":
			_, err = p.flush(ctx, send)
		case "flush_requested":
			if observed.ReloadedCapture == "" {
				_, err = p.ObserveReload(ctx)
			} else {
				_, err = p.PrepareFiles(ctx)
			}
		case "persisted":
			_, err = p.PrepareFiles(ctx)
		case "verified":
			_, err = p.acknowledge(ctx, send)
		case "acknowledged":
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
