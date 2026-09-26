package live

import (
	"context"
	"encoding/json"
	"testing"

	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/live/journal"
)

// A probe whose reported receipt was lost wedges the window: every other exit
// requires evidence that no longer exists. Abandoning from dispatch_requested
// or flush_requested is the honest release - it requires the durable proof that
// the run input was actually queued, then removes the on-disk queue entry and
// releases ownership without game input. flush_requested needs the same escape:
// the host already sent the correlation reload, so no later phase can ever
// reconcile a report that the client never persisted.
func TestAbandonReleasesOperationWithoutReportEvidence(t *testing.T) {
	cases := []struct {
		name      string
		stage     string
		submitted bool
	}{
		{name: "dispatched-with-input", stage: "dispatch_requested", submitted: true},
		{name: "dispatched-without-input", stage: "dispatch_requested", submitted: false},
		{name: "flushed-with-input", stage: "flush_requested", submitted: true},
		{name: "flushed-without-input", stage: "flush_requested", submitted: false},
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
				receipts = map[string]any{"dispatchInput": desktop.InputReceipt{MessagesQueued: 56, SubmissionComplete: true}}
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
			record, err = book.InspectWork(ctx, record.OperationID)
			if err != nil {
				t.Fatal(err)
			}
			_, abandonErr := Abandon(ctx, root, record.OperationID)
			after, _ := book.InspectWork(ctx, record.OperationID)
			if test.submitted {
				if abandonErr != nil || after.Stage != "abandoned" || after.Status != "abandoned" {
					t.Fatalf("abandon: %+v %v", after, abandonErr)
				}
				if _, occupied, err := journal.InspectWindowOwner(ctx, client+"/Interface/AddOns", record.Intent.Resource); err != nil || occupied {
					t.Fatalf("owner not released: %v occupied=%v", err, occupied)
				}
				return
			}
			if abandonErr == nil || after.Stage != test.stage {
				t.Fatalf("unproven input accepted: %+v %v", after, abandonErr)
			}
		})
	}
}
