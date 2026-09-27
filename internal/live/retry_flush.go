package live

import (
	"context"
	"encoding/json"
	"errors"
	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/live/journal"
)

// Zero is evidence only when the sender actually recorded an outcome. Missing,
// partial and successful submission are deliberately not interchangeable.
func unsentFlushIntent(record journal.WorkRecord) bool {
	var observed probeLoadObservation
	return record.Stage == "flush_requested" && record.Status == "unresolved" &&
		json.Unmarshal(record.Observation, &observed) == nil && observed.FlushInput != nil &&
		observed.FlushInput.MessagesQueued == 0 && !observed.FlushInput.SubmissionComplete
}

func (p *ProbeOperation) retryUnsentFlush(ctx context.Context, send preparedInput) (desktop.InputReceipt, error) {
	if err := p.check(ctx); err != nil {
		return desktop.InputReceipt{}, err
	}
	book := journal.OpenBook(p.metadata)
	record, err := book.InspectWork(ctx, p.id)
	if err != nil {
		return desktop.InputReceipt{}, err
	}
	if !unsentFlushIntent(record) {
		return desktop.InputReceipt{}, journal.ErrTransition
	}
	input, _, err := probeDefinition(record)
	if err != nil {
		return desktop.InputReceipt{}, err
	}
	attempt, err := receiverAttemptForWork(ctx, p.metadata, p.id, "reload", input.Expected.RequestID, "-")
	if err != nil {
		return desktop.InputReceipt{}, err
	}
	// Even contradictory zero counters cannot authorize replay past WAKE.
	if attempt.Phase != "wake_requested" || attempt.Nonce != "" || attempt.Attempt >= 3 {
		return desktop.InputReceipt{}, errors.New("live.flush_retry_not_proven_unsent")
	}
	return p.submitInput(ctx, "flush_requested", send, func(ctx context.Context) (string, error) {
		if err := p.prepareFlushReadiness(ctx); err != nil {
			return "", err
		}
		current, err := book.InspectWork(ctx, p.id)
		if err != nil {
			return "", err
		}
		if !unsentFlushIntent(current) {
			return "", journal.ErrTransition
		}
		stage := bridge.ReceiverStage{Action: "reload", RequestID: input.Expected.RequestID, Arg: "-", AttemptID: attempt.AttemptID}
		if err := p.recordReceiverProgress(ctx, "unsent", stage, 0, bridge.Signal{}); err != nil {
			return "", err
		}
		var observed probeLoadObservation
		if err := json.Unmarshal(current.Observation, &observed); err != nil {
			return "", err
		}
		// Invalidate the old zero outcome before any new native input. A crash
		// from here onward must never reuse it as proof about this new attempt.
		observed.FlushInput = nil
		raw, _ := json.Marshal(observed)
		if err := book.AdvanceStage(ctx, journal.StageChange{OperationID: p.id,
			ExpectedGeneration: current.Generation, ExpectedStage: current.Stage,
			Stage: current.Stage, Status: "running", Observation: raw}); err != nil {
			return "", err
		}
		return "/dev bridge reload " + input.Expected.RequestID, nil
	})
}
