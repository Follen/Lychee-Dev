package live

import (
	"encoding/json"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/live/journal"
)

func budgetRecord(t *testing.T, seconds int, stage string, observation []byte) journal.WorkRecord {
	t.Helper()
	request, err := json.Marshal(ReportIntent{Schema: "lycheedev.report-intent.v1", BudgetSeconds: seconds, Expected: bridge.SignalExpectation{SessionNonce: "nonce"}})
	if err != nil {
		t.Fatal(err)
	}
	return journal.WorkRecord{Intent: journal.WorkIntent{Snapshot: "snapshot", Session: "nonce", Request: request}, Stage: stage, Observation: observation}
}

func TestExecutionBudgetIsFrozenAcrossResume(t *testing.T) {
	start := time.Date(2026, 9, 27, 0, 0, 0, 0, time.UTC)
	observation, _ := json.Marshal(probeLoadObservation{ExecutionRequestedAt: start})
	record := budgetRecord(t, 120, "dispatch_requested", observation)
	remaining := executionRemaining(record, start.Add(100*time.Second))
	want := receiverPhaseBudget + 120*time.Second + executionReceiptGrace - 100*time.Second
	if remaining != want {
		t.Fatalf("resume remaining %s, want %s", remaining, want)
	}
	if got := executionRemaining(record, start.Add(201*time.Second)); got >= 0 {
		t.Fatalf("expired execution regained time: %s", got)
	}
	if got := lifecycleBudget(record, start.Add(201*time.Second)); got != resultPersistenceBudget {
		t.Fatalf("expired execution still gets a run: %s", got)
	}
}

func TestExecutionBudgetIncludesReportPersistence(t *testing.T) {
	record := budgetRecord(t, 120, "loaded", nil)
	if got := lifecycleBudget(record, time.Now()); got <= 2*time.Minute {
		t.Fatalf("120 second execution truncated by lifecycle budget: %s", got)
	}
}

func TestLoadBudgetCoversReconnectPrepareAndLoad(t *testing.T) {
	// A selected session is reverified with identity and connect before the
	// prepare and load transactions. Each transaction has its own 60-second
	// bound; the outer load context must not cancel a valid last transaction.
	minimum := 4*receiverPhaseBudget + 2*15*time.Second + resultPersistenceBudget
	if loadPhaseBudget < minimum {
		t.Fatalf("load deadline %s truncates valid reconnect/load path (%s)", loadPhaseBudget, minimum)
	}
}

func TestBootstrapLifecycleBudgetCoversThreeReceiverTransactions(t *testing.T) {
	minimum := 3*receiverPhaseBudget + 3*15*time.Second + resultPersistenceBudget
	if bootstrapLifecycleBudget < minimum {
		t.Fatalf("bootstrap deadline %s truncates first connect, reset, or reload (%s)", bootstrapLifecycleBudget, minimum)
	}
}

func TestDeclaredBudgetValidatedWithAccount(t *testing.T) {
	request := LoadProbeRequest{Session: "SESSION-1", Account: "Account-A", Probe: "probe", Request: "request", BudgetSeconds: 121}
	if err := request.Validate(); err == nil {
		t.Fatal("account bypassed budget validation")
	}
	request.BudgetSeconds = 120
	if err := request.Validate(); err != nil {
		t.Fatal(err)
	}
}
