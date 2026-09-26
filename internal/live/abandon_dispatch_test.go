package live

import (
	"context"
	"encoding/json"
	"testing"

	"github.com/follenfang/lycheedev/internal/delivery"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/live/journal"
)

// Explicit host-only recovery preserves unknown execution for any input receipt.
func TestAbandonReleasesOperationWithoutReportEvidence(t *testing.T) {
	cases := []struct {
		name         string
		stage        string
		submitted    bool
		interrupted  bool
		queueRetired bool
		partial      bool
	}{
		{name: "dispatched-with-input", stage: "dispatch_requested", submitted: true},
		{name: "dispatched-without-input", stage: "dispatch_requested", submitted: false},
		{name: "flushed-with-input", stage: "flush_requested", submitted: true},
		{name: "flushed-without-input", stage: "flush_requested", submitted: false},
		{name: "dispatched-interrupted", stage: "dispatch_requested", submitted: true, interrupted: true},
		{name: "flushed-interrupted", stage: "flush_requested", submitted: true, interrupted: true},
		{name: "retirement-interrupted", stage: "flush_requested", submitted: true, interrupted: true, queueRetired: true},
		{name: "partial-dispatch", stage: "dispatch_requested", submitted: true, partial: true},
	}
	for _, test := range cases {
		t.Run(test.name, func(t *testing.T) {
			ctx := context.Background()
			root, client, book, record, _ := probeOperationFixture(t)
			if _, err := PrepareOperationQueue(ctx, root, record.OperationID); err != nil {
				t.Fatal(err)
			}
			record, err := book.InspectWork(ctx, record.OperationID)
			if err != nil {
				t.Fatal(err)
			}
			if record.Stage != "load_requested" {
				t.Fatalf("fixture stage %q, want load_requested", record.Stage)
			}
			base := map[string]any{
				"schema":   "lycheedev.probe-load.v1",
				"revision": map[string]any{"sha256": "34e060b2218d7a6564b144e38a05061ab5da748ab28624ac5ef84d3b143ea357", "entries": 1},
			}
			advance := func(stage string, extra map[string]any) {
				t.Helper()
				observation := map[string]any{}
				for key, value := range base {
					observation[key] = value
				}
				for key, value := range extra {
					observation[key] = value
				}
				raw, err := json.Marshal(observation)
				if err != nil {
					t.Fatal(err)
				}
				latest, err := book.InspectWork(ctx, record.OperationID)
				if err != nil {
					t.Fatal(err)
				}
				if err := book.AdvanceStage(ctx, journal.StageChange{
					OperationID: record.OperationID, ExpectedGeneration: latest.Generation,
					ExpectedStage: latest.Stage, Stage: stage, Status: "running", Observation: raw,
				}); err != nil {
					t.Fatal(err)
				}
			}
			advance("loaded", nil)
			// The durable receipt chain accumulates: a later stage keeps the
			// earlier dispatch receipt, which is the proof abandon relies on.
			var receipts map[string]any
			if test.submitted {
				receipts = map[string]any{"dispatchInput": desktop.InputReceipt{MessagesQueued: 56, SubmissionComplete: !test.partial}}
			}
			advance("dispatch_requested", receipts)
			if test.stage == "flush_requested" {
				next := map[string]any{
					"reportedCapture": "CAP-reported",
					"flushInput":      desktop.InputReceipt{MessagesQueued: 59, SubmissionComplete: true},
				}
				for key, value := range receipts {
					next[key] = value
				}
				advance("reported", next)
				advance("flush_requested", receipts)
			}
			if test.interrupted {
				advance("abandoning", receipts)
			}
			record, err = book.InspectWork(ctx, record.OperationID)
			if err != nil {
				t.Fatal(err)
			}
			if test.queueRetired {
				input, definition, err := probeDefinition(record)
				if err != nil {
					t.Fatal(err)
				}
				if _, err := delivery.ChangeProbeQueue(ctx, delivery.AddonDirectory(input.Load.Installation), definition, true); err != nil {
					t.Fatal(err)
				}
			}
			_, abandonErr := Abandon(ctx, root, record.OperationID)
			after, _ := book.InspectWork(ctx, record.OperationID)
			if abandonErr != nil || after.Stage != "abandoned" || after.Status != "abandoned" {
				t.Fatalf("abandon: %+v %v", after, abandonErr)
			}
			if _, occupied, err := journal.InspectWindowOwner(ctx, client+"/Interface/AddOns", record.Intent.Resource); err != nil || occupied {
				t.Fatalf("owner not released: %v occupied=%v", err, occupied)
			}
			var retained probeLoadObservation
			if err := json.Unmarshal(after.Observation, &retained); err != nil || test.submitted && (retained.DispatchInput == nil || retained.DispatchInput.SubmissionComplete == test.partial) {
				t.Fatalf("dispatch evidence lost: %s %v", after.Observation, err)
			}
			for _, call := range []func(context.Context, string, string) (Outcome, error){Abandon, Resume} {
				out, err := call(ctx, root, record.OperationID)
				if err != nil || out.Report.State != "unavailable" || out.Cleanup != "abandoned" || out.Complete {
					t.Fatalf("repeat/recovery: %+v %v", out, err)
				}
			}
		})
	}
}
