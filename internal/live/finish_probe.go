package live

import (
	"context"
	"errors"
)

// FinishOutcome includes display verification without changing the meaning of
// atomic ACK/status. A verified report is retained when display cleanup fails.
type FinishOutcome struct {
	Outcome
	Display DisplayOutcome `json:"display"`
}

type DisplayOutcome struct {
	State   string             `json:"state"`
	Capture string             `json:"capture,omitempty"`
	Receipt *HideReceiptResult `json:"receipt,omitempty"`
}

// FinishProbe closes the user-visible task. ACK is idempotent and checked before
// hide, so retrying after a lost clear only reconnects and clears the display.
func FinishProbe(ctx context.Context, root, id string) (FinishOutcome, error) {
	return finishProbe(ctx, root, id, AcknowledgeVerified, HideReceipt)
}

func finishProbe(ctx context.Context, root, id string,
	acknowledge func(context.Context, string, string) (Outcome, error),
	hide func(context.Context, string, string) (HideReceiptResult, error),
) (FinishOutcome, error) {
	result := FinishOutcome{Display: DisplayOutcome{State: "pending"}}
	outcome, err := acknowledge(ctx, root, id)
	result.Outcome = outcome
	result.Complete = false
	if err != nil {
		return result, err
	}
	if outcome.Report.State != "verified" || outcome.Cleanup != "complete" || !outcome.Complete {
		return result, errors.New("live.finish_requires_acknowledgement")
	}
	record, err := InspectOperation(ctx, root, id)
	if err != nil {
		return result, err
	}
	input, err := reportInput(record)
	if err != nil {
		return result, err
	}
	if input.Binding == "" {
		return result, errors.New("live.session_required")
	}
	if display, found, err := readDisplayCompletion(ctx, root, record, input.Binding); err != nil {
		return result, err
	} else if found {
		result.Display, result.Complete = display, true
		return result, nil
	}
	receipt, err := hide(ctx, root, input.Binding)
	result.Display.Receipt = &receipt
	if err != nil {
		return result, err
	}
	if !receipt.Cleared {
		return result, ErrReceiptHidePending
	}
	display, err := saveDisplayCompletion(ctx, root, record, input.Binding, receipt)
	if err != nil {
		return result, err
	}
	result.Display = display
	result.Complete = true
	return result, nil
}
