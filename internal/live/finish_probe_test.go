package live

import (
	"context"
	"errors"
	"testing"
	"time"
)

// ACK and display verification have independent failure boundaries. The
// orchestration must never suppress the report or turn a missing clear into
// success, and retries must keep the original operation and window binding.
func TestFinishRetainsReportUntilDisplayVerified(t *testing.T) {
	ctx := context.Background()
	root, _, _, record, input := probeOperationFixture(t)
	ackCalls, hideCalls := 0, 0
	outcome := Outcome{OperationID: record.OperationID, Complete: true, Report: ReportOutcome{State: "verified", Content: []byte(`{"answer":42}`)}, Cleanup: "complete"}
	ack := func(_ context.Context, gotRoot, id string) (Outcome, error) {
		if gotRoot != root || id != record.OperationID {
			t.Fatal("finish retargeted operation")
		}
		ackCalls++
		return outcome, nil
	}
	hide := func(_ context.Context, gotRoot, session string) (HideReceiptResult, error) {
		if gotRoot != root || session != input.Binding {
			t.Fatal("finish retargeted session")
		}
		hideCalls++
		if hideCalls == 1 {
			return HideReceiptResult{Session: session}, ErrReceiptHidePending
		}
		return HideReceiptResult{Session: session, Cleared: true, ObservedAt: time.Now()}, nil
	}
	first, err := finishProbe(ctx, root, record.OperationID, ack, hide)
	if !errors.Is(err, ErrReceiptHidePending) || first.Complete || first.Report.State != "verified" || first.Cleanup != "complete" || first.Display.State != "pending" {
		t.Fatalf("lost clear: %+v %v", first, err)
	}
	final, err := finishProbe(ctx, root, record.OperationID, ack, hide)
	if err != nil || !final.Complete || final.Display.State != "cleared" || ackCalls != 2 || hideCalls != 2 {
		t.Fatalf("retry: %+v %v", final, err)
	}
	repeated, err := finishProbe(ctx, root, record.OperationID, ack, hide)
	if err != nil || !repeated.Complete || hideCalls != 2 || repeated.Display.Capture != final.Display.Capture {
		t.Fatalf("successful finish replayed input: %+v %v", repeated, err)
	}
	outcome.Cleanup = "pending"
	outcome.Complete = false
	pending, err := finishProbe(ctx, root, record.OperationID, ack, hide)
	if err == nil || pending.Complete || hideCalls != 2 {
		t.Fatal("finish hid an unacknowledged report")
	}
}
