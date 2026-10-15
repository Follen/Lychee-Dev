package channel

import (
	"context"
	"encoding/json"
	"errors"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/live/journal"
)

// Every invocation gets a checksum-valid fresh INPUT. The fake sender models
// the native held-key/quiet-period check: zero queued messages is positively known.
// Cancelling after that observation gives one bounded CLI turn without waiting
// for wall time or posting a key to any window.
type heldKeyJournalPeer struct {
	pendingBackend
	t                      *testing.T
	clock                  *time.Time
	start                  time.Time
	turnCancel             context.CancelFunc
	attempts, observations int
	uncertain              bool
}

func (p *heldKeyJournalPeer) uptime() int64 { return 1000000 + p.clock.Sub(p.start).Milliseconds() }
func (p *heldKeyJournalPeer) ObserveInput(_ context.Context, e bridge.SlotEnvelope, after int64, _ string) (InputObservation, error) {
	p.observations++
	blocked := false
	s := InputObservation{Schema: InputSchema, Bindings: primaryInputBindings(), Runtime: e.Runtime, Owner: e.Owner, Fence: e.Fence, NextSlot: slotStart(e), GUID: e.GUID, Build: e.Build, SampleMillis: p.uptime(), InputBlocked: &blocked}
	return inputObservation(inputTestRecord(p.t, s), e, after, p.uptime())
}
func (p *heldKeyJournalPeer) Input(context.Context, InputAction) (InputOutcome, error) {
	p.attempts++
	p.turnCancel()
	if p.uncertain {
		return InputOutcome{Disposition: "uncertain", MessagesQueued: 1, AtMillis: p.uptime(), Reason: "fixture_submission_unknown"}, errors.New("fixture_submission_unknown")
	}
	blocker := desktop.ErrKeyAlreadyHeld
	if p.attempts%2 != 0 {
		blocker = desktop.ErrKeyboardActive
	}
	return InputOutcome{Disposition: "not_sent", AtMillis: p.uptime(), Retryable: true, Reason: blocker.Error()}, blocker
}
func (p *heldKeyJournalPeer) Observe(context.Context, ObservationQuery) (Observation, error) {
	p.reads++
	p.turnCancel()
	return Observation{}, ErrPending
}
func heldKeyTurn(d *Driver, p *heldKeyJournalPeer) error {
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	p.turnCancel = cancel
	return d.Continue(ctx)
}

func TestHeldKeyRetriesKeepJournalCapacityAndOriginalDeadline(t *testing.T) {
	ctx := context.Background()
	start := time.Unix(1800000000, 0)
	now := start
	p := &heldKeyJournalPeer{t: t, clock: &now, start: start}
	i := Identity{Schema: IdentitySchema, InputState: InputSchema, Runtime: strings.Repeat("1", 32), NextSlot: 4, Slots: 500, GUID: "g", Character: "c", Realm: "r", Build: "b", Product: "retail", Release: "3.2.0"}
	d, err := New(filepath.Join(t.TempDir(), "connections", "held.jsonl"), p, i)
	if err != nil {
		t.Fatal(err)
	}
	d.Now = func() time.Time { return now }
	d.State.Bound = true
	if err := d.PrepareRequest(ctx, "held-key-original", "return 1", 5, "opaque"); err != nil {
		t.Fatal(err)
	}
	request := *d.State.Operation
	deadline := request.RecoveryBudget.DeadlineMS
	if deadline-start.UnixMilli() != DefaultRecoveryBudget.Milliseconds() {
		t.Fatal("fixture did not use the original 600 second budget")
	}
	nonce := ""
	completed := 0
	var turnErr error
	for turn := 0; turn < 600; turn++ {
		now = now.Add(500 * time.Millisecond) // Freshness is 100ms; this is slower than the 250ms scheduler.
		d, err = Load(d.Log, p)
		if err != nil {
			t.Fatal(err)
		}
		d.Now = func() time.Time { return now }
		turnErr = heldKeyTurn(d, p)
		if !errors.Is(turnErr, ErrPending) {
			break
		}
		if d.State.Operation.ID != request.ID || d.State.Operation.Ticket != request.Ticket || d.State.Operation.RecoveryBudget.DeadlineMS != deadline || d.State.Operation.RecoveryBudget.Exhausted {
			t.Fatal("resume replaced request or renewed deadline")
		}
		if d.State.Transaction == nil || d.State.Transaction.Phase != "published" {
			break
		}
		if nonce == "" {
			nonce = d.State.Transaction.Envelope.Nonce
		}
		if d.State.Transaction.Envelope.Nonce != nonce {
			t.Fatal("held key changed the published nonce")
		}
		completed++
	}
	if completed == 600 {
		// The first attempt after those archived zero-input retries becomes
		// uncertain. Further resumes must only read its original receipt.
		p.uncertain = true
		now = now.Add(500 * time.Millisecond)
		d, err = Load(d.Log, p)
		if err != nil {
			t.Fatal(err)
		}
		d.Now = func() time.Time { return now }
		if err := heldKeyTurn(d, p); err == nil || !strings.Contains(err.Error(), "fixture_submission_unknown") {
			t.Fatal("missing unknown submission", err)
		}
		inputID := d.State.Input.ID
		for retry := 0; retry < 2; retry++ {
			d, err = Load(d.Log, p)
			if err != nil {
				t.Fatal(err)
			}
			d.Now = func() time.Time { return now }
			if err := heldKeyTurn(d, p); !errors.Is(err, ErrPending) {
				t.Fatal(err)
			}
			if p.attempts != 601 || d.State.Input.ID != inputID {
				t.Fatal("archived retry history licensed unknown-input replay")
			}
		}
		now = time.UnixMilli(deadline)
		if err := heldKeyTurn(d, p); !errors.Is(err, ErrBudgetExhausted) {
			t.Fatal("original 600 second deadline did not stop recovery", err)
		}
		if p.attempts != 601 || !d.State.Operation.RecoveryBudget.Exhausted {
			t.Fatal("expired goal issued input or renewed budget")
		}

	}

	// Before the fix this is a concrete end-to-end failure, not arithmetic about
	// limits: the active segment has 1024 events and even close intent cannot save.
	closeErr := d.RequestClose(ctx)
	if completed != 600 || closeErr != nil {
		events, readErr := journal.ReadMemoryLog(d.Log)
		t.Fatalf("held-key resumes=%d attempts=%d elapsed=%s remaining=%s events=%d read=%v continuation=%v close=%v", completed, p.attempts, now.Sub(start), time.Duration(deadline-now.UnixMilli())*time.Millisecond, len(events), readErr, turnErr, closeErr)
	}
	if p.attempts != 601 || p.observations != 601 || p.publishes != 1 {
		t.Fatalf("unexpected physical attempts/reads/publications: %+v", p)
	}
	if d.State.Operation.RecoveryBudget.DeadlineMS != deadline {
		t.Fatal("close changed the business deadline")
	}
	loaded, err := Load(d.Log, p)
	if err != nil || !loaded.State.Closing {
		t.Fatalf("close intent was not durable: %v", err)
	}
	loaded.Now = func() time.Time { return now }
	if err := heldKeyTurn(loaded, p); !errors.Is(err, ErrPending) {
		t.Fatal(err)
	}
	if p.attempts != 601 || p.publishes != 1 || loaded.Waiting != "closing_exchange_unconfirmed" {
		t.Fatal("close replayed the published prepare")
	}
	// Traverse the immutable predecessor chain and check that compaction retained
	// every known-zero outcome and its original exchange, not merely a counter.
	path := loaded.Log
	expected := ""
	intents, outcomes, segments, zeroOutcomes := 0, 0, 0, 0
	for {
		events, err := journal.ReadMemoryLog(path)
		if err != nil || len(events) == 0 {
			t.Fatalf("history: %v", err)
		}
		if len(events) >= 1024 {
			t.Fatal("active/archived segment exhausted its hard limit")
		}
		if expected != "" && events[len(events)-1].Checksum != expected {
			t.Fatal("history predecessor changed")
		}
		for _, event := range events {
			if event.Kind != "input_intent" && event.Kind != "input_observed" {
				continue
			}
			var state State
			if err := json.Unmarshal(event.Data, &state); err != nil {
				t.Fatal(err)
			}
			if state.Input == nil || state.Input.Exchange != nonce || state.Operation.ID != request.ID || state.Operation.RecoveryBudget.DeadlineMS != deadline {
				t.Fatal("archival changed input identity or budget")
			}
			if event.Kind == "input_intent" {
				intents++
			} else {
				outcomes++
				if state.Input.Outcome == nil {
					t.Fatal("lost exact input outcome")
				}
				if state.Input.Outcome.Disposition == "not_sent" && state.Input.Outcome.MessagesQueued == 0 {
					zeroOutcomes++
				} else if state.Input.Outcome.Disposition != "uncertain" || state.Input.Outcome.MessagesQueued != 1 {
					t.Fatal("changed unknown input outcome")
				}
			}
		}
		var latest State
		if err := json.Unmarshal(events[len(events)-1].Data, &latest); err != nil {
			t.Fatal(err)
		}
		segments++
		if latest.Archive == "" {
			break
		}
		expected = latest.Archive
		path = filepath.Join(loaded.Log+".history", expected+".jsonl")
		if segments > 16 {
			t.Fatal("history loop")
		}
	}
	if intents != 601 || outcomes != 601 || zeroOutcomes != 600 || segments < 2 {
		t.Fatalf("history: intents=%d outcomes=%d segments=%d", intents, outcomes, segments)
	}
	t.Logf("600 bounded retries and unknown-input resumes; at elapsed %s original budget stops; %d immutable segments retained %d known-zero outcomes; close intent persisted without replay", now.Sub(start), segments, zeroOutcomes)
}
