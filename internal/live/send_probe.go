package live

import (
	"context"
	"encoding/json"
	"errors"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"time"
)

type preparedInput func(context.Context, desktop.WindowIdentity, func(context.Context) (string, error), func(context.Context) error) (desktop.InputReceipt, error)

// Dispatch obtains a new loaded receipt while owning the native input mutex,
// persists dispatch intent, then queues exactly one command. The result proves
// only input submission; Observe must establish execution/report evidence.
// Partial or failed input after intent is never automatically replayed.
func (p *ProbeOperation) Dispatch(ctx context.Context) (desktop.InputReceipt, error) {
	return p.dispatch(ctx, desktop.QueuePreparedCommand)
}

func (p *ProbeOperation) dispatch(ctx context.Context, send preparedInput) (desktop.InputReceipt, error) {
	if err := p.check(ctx); err != nil {
		return desktop.InputReceipt{}, err
	}
	record, err := journal.OpenBook(p.metadata).InspectWork(ctx, p.id)
	if err != nil {
		return desktop.InputReceipt{}, err
	}
	if record.Stage != "load_requested" && record.Stage != "loaded" {
		return desktop.InputReceipt{}, journal.ErrTransition
	}
	return p.submitInput(ctx, "dispatch_requested", send, func(ctx context.Context) (string, error) {
		if _, err := p.Observe(ctx); err != nil {
			return "", err
		}
		if err := p.check(ctx); err != nil {
			return "", err
		}
		return RequestOperationDispatch(ctx, p.root, p.id)
	})
}

func (p *ProbeOperation) submitInput(ctx context.Context, stage string, send preparedInput, prepare func(context.Context) (string, error)) (desktop.InputReceipt, error) {
	if send == nil {
		return desktop.InputReceipt{}, errors.New("live.input_sender_missing")
	}
	firstInput := false
	intentPrepared := false
	guard := func(ctx context.Context) error {
		if err := p.check(ctx); err != nil {
			return err
		}
		if firstInput {
			if err := p.session.reader.RequireFreshSignal(); err != nil {
				return err
			}
			// Chat opening invalidates the optical readiness receipt. Subsequent
			// messages retain ownership checks, not a requirement for that old QR.
			firstInput = false
		}
		return nil
	}
	receipt, sendErr := send(ctx, p.session.target.Window, func(ctx context.Context) (string, error) {
		command, err := prepare(ctx)
		if err == nil {
			firstInput = true
			intentPrepared = true
		}
		return command, err
	}, guard)
	if !intentPrepared {
		return receipt, sendErr
	}
	// Cancellation or partial submission must remain inspectable. This bounded
	// metadata-only write cannot send input or release window ownership.
	persist, cancel := context.WithTimeout(context.WithoutCancel(ctx), 2*time.Second)
	defer cancel()
	book := journal.OpenBook(p.metadata)
	current, err := book.InspectWork(persist, p.id)
	if err != nil {
		return receipt, errors.Join(sendErr, err)
	}
	if current.Stage != stage {
		return receipt, sendErr
	}
	var raw []byte
	if stage == "ack_requested" || stage == "acknowledged" {
		var observed reportObservation
		if err := json.Unmarshal(current.Observation, &observed); err != nil {
			return receipt, errors.Join(sendErr, err)
		}
		if stage == "acknowledged" {
			if observed.CleanupReadyID == "" {
				return receipt, sendErr
			}
			observed.CleanupInput = &receipt
		} else {
			observed.AckInput = &receipt
		}
		raw, _ = json.Marshal(observed)
	} else {
		var observed probeLoadObservation
		if err := json.Unmarshal(current.Observation, &observed); err != nil {
			return receipt, errors.Join(sendErr, err)
		}
		if stage == "load_requested" {
			if observed.LoadReadyCapture != "" {
				observed.LoadInput = &receipt
			} else if observed.BootstrapReadyID != "" {
				observed.BootstrapInput = &receipt
			} else {
				return receipt, sendErr
			}
		} else if stage == "dispatch_requested" {
			observed.DispatchInput = &receipt
		} else if stage == "flush_requested" {
			observed.FlushInput = &receipt
		} else {
			return receipt, errors.Join(sendErr, journal.ErrTransition)
		}
		raw, _ = json.Marshal(observed)
	}
	status := "running"
	if sendErr != nil {
		status = "unresolved"
	}
	err = book.AdvanceStage(persist, journal.StageChange{OperationID: p.id, ExpectedGeneration: current.Generation, ExpectedStage: stage, Stage: stage, Status: status, Observation: raw})
	return receipt, errors.Join(sendErr, err)
}
