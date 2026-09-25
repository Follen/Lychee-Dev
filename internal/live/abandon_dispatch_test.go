package live

import (
	"context"
	"encoding/json"
	"testing"

	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/live/journal"
)

// A dispatched probe whose reported receipt was lost (hidden card, client
// restart) wedges the window: every other exit requires evidence that no
// longer exists. Abandoning from dispatch_requested is the honest release -
// it requires the durable proof that the queue input was actually sent, then
// removes the on-disk queue entry and releases ownership without input.
func TestAbandonReleasesDispatchedOperation(t *testing.T) {
	for _, submitted := range []bool{true, false} {
		t.Run(submittedLabel(submitted), func(t *testing.T) {
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
			if submitted {
				advance("dispatch_requested", map[string]any{
					"dispatchInput": desktop.InputReceipt{MessagesQueued: 56, SubmissionComplete: true},
				})
			} else {
				advance("dispatch_requested", nil)
			}
			record, err = book.InspectWork(ctx, record.OperationID)
			if err != nil {
				t.Fatal(err)
			}
			var abandonErr error
			_, abandonErr = Abandon(ctx, root, record.OperationID)
			after, _ := book.InspectWork(ctx, record.OperationID)
			if submitted {
				if abandonErr != nil || after.Stage != "abandoned" || after.Status != "abandoned" {
					t.Fatalf("dispatched abandon: %+v %v", after, abandonErr)
				}
				if _, occupied, err := journal.InspectWindowOwner(ctx, client+"/Interface/AddOns", record.Intent.Resource); err != nil || occupied {
					t.Fatalf("owner not released: %v occupied=%v", err, occupied)
				}
			} else {
				if abandonErr == nil || after.Stage != "dispatch_requested" {
					t.Fatalf("unproven dispatch accepted: %+v %v", after, abandonErr)
				}
			}
		})
	}
}

func submittedLabel(submitted bool) string {
	if submitted {
		return "dispatch-input-submitted"
	}
	return "no-dispatch-input"
}
