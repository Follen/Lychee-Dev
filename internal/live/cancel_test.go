package live

import (
	"context"
	"errors"
	"testing"

	"github.com/follenfang/lycheedev/internal/live/journal"
)

func TestCancelPreparedReleasesWindowWithoutGameInput(t *testing.T) {
	root, client, book, record, _ := probeOperationFixture(t)
	result, err := Cancel(context.Background(), root, record.OperationID)
	if err != nil || result.Status != "cancelled" || result.Cleanup != "complete" || result.Report.State != "unavailable" || result.Complete {
		t.Fatalf("cancel result: %+v %v", result, err)
	}
	stored, err := book.InspectWork(context.Background(), record.OperationID)
	if err != nil || stored.Stage != "cleaned" || stored.Status != "cancelled" {
		t.Fatalf("work record: %+v %v", stored, err)
	}
	if owner, found, err := journal.InspectWindowOwner(context.Background(), client+"/Interface/AddOns", record.Intent.Resource); err != nil || found {
		t.Fatalf("window owner retained: %+v %v %v", owner, found, err)
	}
	again, err := Cancel(context.Background(), root, record.OperationID)
	if err != nil || again.Status != "cancelled" {
		t.Fatalf("idempotent cancellation: %+v %v", again, err)
	}
	resumed, err := Resume(context.Background(), root, record.OperationID)
	if err != nil || resumed.Status != "cancelled" {
		t.Fatalf("resuming cancelled work sent input or failed: %+v %v", resumed, err)
	}
}

func TestCancelRefusesPublishedQueue(t *testing.T) {
	root, _, book, record, _ := probeOperationFixture(t)
	if _, err := PrepareOperationQueue(context.Background(), root, record.OperationID); err != nil {
		t.Fatal(err)
	}
	result, err := Cancel(context.Background(), root, record.OperationID)
	if !errors.Is(err, journal.ErrTransition) || result.OperationID != record.OperationID {
		t.Fatalf("published queue cancellation: %+v %v", result, err)
	}
	stored, err := book.InspectWork(context.Background(), record.OperationID)
	if err != nil || stored.Stage != "load_requested" || stored.Status == "cancelled" {
		t.Fatalf("work changed by refused cancellation: %+v %v", stored, err)
	}
}
