package channel

import (
	"context"
	"encoding/json"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/live/journal"
)

func TestClosedContinuationSurvivesCrashBeforeBlockerClear(t *testing.T) {
	d, p := stopReconcileFixture(t)
	d.State.Blocker = &Blocker{Kind: "input_combat", Condition: "input_condition_changed"}
	if err := d.RequestClose(context.Background()); err != nil {
		t.Fatal(err)
	}
	if err := stopReconcileContinueFor(d, 5*time.Second); err != nil {
		t.Fatal(err)
	}
	// Replay the real durable prefix ending at connection_closed. This is a
	// crash after successful close, before Continue's deferred blocker clear.
	data, err := os.ReadFile(d.Log)
	if err != nil {
		t.Fatal(err)
	}
	lines := strings.Split(strings.TrimSuffix(string(data), "\n"), "\n")
	cut := -1
	for i, line := range lines {
		var event journal.MemoryEvent
		if err = json.Unmarshal([]byte(line), &event); err != nil {
			t.Fatal(err)
		}
		if event.Kind == "connection_closed" {
			cut = i + 1
			break
		}
	}
	if cut < 0 {
		t.Fatal("no actual terminal close event")
	}
	if err = os.WriteFile(d.Log, []byte(strings.Join(lines[:cut], "\n")+"\n"), 0600); err != nil {
		t.Fatal(err)
	}
	resumed, err := Load(d.Log, p)
	if err != nil {
		t.Fatal(err)
	}
	if !resumed.State.Closed {
		t.Fatal("terminal close not persisted")
	}
	if c := resumed.continuation(); c.Kind != "completed" {
		t.Fatalf("successful close requires resuming a historical blocker: %+v", c)
	}
}
