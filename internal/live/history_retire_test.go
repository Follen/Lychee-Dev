package live

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/vault"
	"hash/adler32"
	"os"
	"path/filepath"
	"testing"
)

func TestLegacyRetirementPreservesIntentAndEvidence(t *testing.T) {
	for _, mode := range []string{"prepared", "uncertain", "verified"} {
		t.Run(mode, func(t *testing.T) {
			ctx := context.Background()
			root := filepath.Join(t.TempDir(), "workspace")
			store, err := vault.Initialize(ctx, root)
			if err != nil {
				t.Fatal(err)
			}
			game := t.TempDir()
			parent := filepath.Join(game, "Interface", "AddOns")
			if err := os.MkdirAll(parent, 0700); err != nil {
				t.Fatal(err)
			}
			m, err := store.OpenMetadata(ctx)
			if err != nil {
				t.Fatal(err)
			}
			t.Cleanup(func() { m.Close() })
			book := journal.OpenBook(m)
			signal := bridge.Signal{Schema: "lycheedev.signal.v1", Release: "2.0.0-dev", Kind: "reported", SessionNonce: "session", RequestID: "request", Character: "paladin", Realm: "realm", Product: "retail", Build: "12.1.0.69875", Sequence: 1}
			expected := bridge.SignalExpectation{Kind: signal.Kind, Release: signal.Release, SessionNonce: signal.SessionNonce, RequestID: signal.RequestID, Character: signal.Character, Realm: signal.Realm, Product: signal.Product, Build: signal.Build}
			input := ReportIntent{Schema: "lycheedev.report-intent.v1", Expected: expected, Load: &ProbeLoadIntent{Installation: game}}
			request, _ := json.Marshal(input)
			record, err := book.BeginWindowWork(ctx, parent, store.Identity().WorkspaceID, journal.WorkIntent{Kind: "probe", Resource: "window/1/2/3", Snapshot: "PIN-test", Session: "session", Request: request})
			if err != nil {
				t.Fatal(err)
			}
			var observation = json.RawMessage(`null`)
			if mode != "prepared" {
				observation = json.RawMessage(`{"dispatchInput":{"messagesQueued":1,"submissionComplete":false},"unknownFutureField":"keep"}`)
				if mode == "verified" {
					body := []byte(`{ "answer":42 }`)
					signal.ReportBytes = uint32(len(body))
					signal.ReportAdler32 = fmt.Sprintf("%08x", adler32.Checksum(body))
					receipt, _ := json.Marshal(signal)
					pair, err := evidence.OpenArchive(store, m).CommitReport(ctx, receipt, body, nil, expected, record.OperationID, record.Intent.Snapshot)
					if err != nil {
						t.Fatal(err)
					}
					observation, _ = json.Marshal(reportObservation{BodyID: pair.Body.ID, ReceiptID: pair.Receipt.ID})
				}
				if err := book.AdvanceStage(ctx, journal.StageChange{OperationID: record.OperationID, ExpectedGeneration: record.Generation, ExpectedStage: record.Stage, Stage: "load_requested", Status: "running", Observation: observation}); err != nil {
					t.Fatal(err)
				}
			}
			if err := m.Close(); err != nil {
				t.Fatal(err)
			}
			if mode != "prepared" {
				if _, err := Cancel(ctx, root, record.OperationID); !errors.Is(err, journal.ErrTransition) {
					t.Fatalf("cancel uncertain: %v", err)
				}
			}
			for i := 0; i < 2; i++ {
				var result Outcome
				if mode == "prepared" {
					result, err = Cancel(ctx, root, record.OperationID)
				} else {
					result, err = Abandon(ctx, root, record.OperationID)
				}
				if err != nil {
					t.Fatal(err)
				}
				if mode == "verified" && (result.Report.State != "verified" || string(result.Report.Content) != `{ "answer":42 }` || result.Complete) {
					t.Fatalf("lost report or invented game cleanup: %+v", result)
				}
			}
			after, err := vault.ReadWorkspace(ctx, root, func(_ *vault.Store, metadata *vault.Metadata) (journal.WorkRecord, error) {
				return journal.OpenBook(metadata).InspectWork(ctx, record.OperationID)
			})
			if err != nil {
				t.Fatal(err)
			}
			if !bytes.Equal(after.Intent.Request, record.Intent.Request) || !bytes.Equal(after.Observation, observation) {
				t.Fatalf("retirement changed evidence: %s", after.Observation)
			}
			if _, busy, err := journal.InspectWindowOwner(ctx, parent, record.Intent.Resource); err != nil || busy {
				t.Fatalf("owner retained: %v %v", busy, err)
			}
		})
	}
}
