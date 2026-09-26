package live

import (
	"context"
	"encoding/json"
	"path/filepath"
	"testing"

	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/live/journal"
)

func TestAbandonPreDispatchAndUnarchivedProbe(t *testing.T) {
	for _, stop := range []string{"load_requested", "loaded", "reported", "persisted"} {
		t.Run(stop, func(t *testing.T) {
			ctx := context.Background()
			root, client, book, record, _ := probeOperationFixture(t)
			if _, err := PrepareOperationQueue(ctx, root, record.OperationID); err != nil {
				t.Fatal(err)
			}
			record, _ = book.InspectWork(ctx, record.OperationID)
			for _, stage := range []string{"loaded", "dispatch_requested", "reported", "flush_requested", "persisted"} {
				if record.Stage == stop {
					break
				}
				if err := book.AdvanceStage(ctx, journal.StageChange{OperationID: record.OperationID, ExpectedGeneration: record.Generation, ExpectedStage: record.Stage, Stage: stage, Status: "running", Observation: record.Observation}); err != nil {
					t.Fatal(err)
				}
				record, _ = book.InspectWork(ctx, record.OperationID)
			}
			for _, recover := range []func(context.Context, string, string) (Outcome, error){Abandon, Resume, Abandon} {
				out, err := recover(ctx, root, record.OperationID)
				if err != nil || out.Report.State != "unavailable" || out.Cleanup != "abandoned" || out.Complete {
					t.Fatalf("recovery: %+v %v", out, err)
				}
			}
			if _, occupied, err := journal.InspectWindowOwner(ctx, filepath.Join(client, "Interface", "AddOns"), record.Intent.Resource); err != nil || occupied {
				t.Fatal("owner retained", err)
			}
			if len(operationQueue(t, client)) != 0 {
				t.Fatal("queue retained")
			}
		})
	}
}

func TestFaultsExplicitRecoveryRetainsUnknownInput(t *testing.T) {
	for name, receipt := range map[string]*desktop.InputReceipt{"missing": nil, "zero": {}, "partial": {MessagesQueued: 3}, "submitted": {MessagesQueued: 40, SubmissionComplete: true}} {
		t.Run(name, func(t *testing.T) {
			ctx := context.Background()
			root, client, book, pin, session, _ := unpreparedProbeFixture(t, 7)
			defer session.Close()
			record, err := session.prepareFaults(ctx, root, pin.ID, BugsRequest{Session: "saved", Account: "Account-A", Request: "recover", Count: 2})
			if err != nil {
				t.Fatal(err)
			}
			observation, _ := json.Marshal(faultObservation{Schema: "lycheedev.fault-observation.v1", DispatchInput: receipt})
			if err := book.AdvanceStage(ctx, journal.StageChange{OperationID: record.OperationID, ExpectedGeneration: record.Generation, ExpectedStage: "prepared", Stage: "dispatch_requested", Status: "unresolved", Observation: observation}); err != nil {
				t.Fatal(err)
			}
			for _, recover := range []func(context.Context, string, string) (Outcome, error){Abandon, Resume, Abandon} {
				out, err := recover(ctx, root, record.OperationID)
				if err != nil || out.Report.State != "unavailable" || out.Cleanup != "abandoned" || out.Complete {
					t.Fatalf("recovery: %+v %v", out, err)
				}
			}
			if _, occupied, err := journal.InspectWindowOwner(ctx, filepath.Join(client, "Interface", "AddOns"), record.Intent.Resource); err != nil || occupied {
				t.Fatal("owner retained", err)
			}
			stored, _ := book.InspectWork(ctx, record.OperationID)
			var got faultObservation
			if err := json.Unmarshal(stored.Observation, &got); err != nil {
				t.Fatal(err)
			}
			if (got.DispatchInput == nil) != (receipt == nil) || receipt != nil && *got.DispatchInput != *receipt {
				t.Fatal("input evidence changed")
			}
		})
	}
}
