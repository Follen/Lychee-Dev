package channel

import (
	"context"
	"github.com/follenfang/lycheedev/internal/bridge"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

type coordinatorPeer struct {
	recoveryPeer
	log     string
	t       *testing.T
	kinds   []string
	cancel  context.CancelFunc
	blocked int
}

func (p *coordinatorPeer) ObserveInput(ctx context.Context, e bridge.SlotEnvelope, after int64) (InputObservation, error) {
	s, err := p.recoveryPeer.ObserveInput(ctx, e, after)
	blocked := p.blocked > 0
	s.InputBlocked = &blocked
	if blocked {
		s.Reason = "input_keyboard_focus"
	}
	return s, err
}

func (p *coordinatorPeer) Input(ctx context.Context, a InputAction) (InputOutcome, error) {
	persisted, err := Load(p.log, p)
	if err != nil || persisted.State.Input == nil || persisted.State.Input.Kind != a.Kind || persisted.State.Input.Outcome != nil {
		p.t.Fatalf("physical input without durable intent: %v", err)
	}
	p.kinds = append(p.kinds, a.Kind)
	if a.Kind == "escape" {
		p.blocked--
	}
	if p.cancel != nil {
		p.cancel()
	}
	return InputOutcome{Disposition: "submitted", MessagesQueued: 2}, nil
}
func TestReloadUsesOneCoordinatorAndNeverReplaysLostOutcome(t *testing.T) {
	for _, mode := range []string{"new", "lost", "submitted", "zero", "focused"} {
		t.Run(mode, func(t *testing.T) {
			old := Identity{Runtime: strings.Repeat("1", 32), NextSlot: 5, Slots: 64, GUID: "g", Character: "c", Realm: "r", Build: "b", Product: "retail", Release: "2.5.1"}
			current := old
			current.Runtime = strings.Repeat("2", 32)
			current.NextSlot = 1
			p := &coordinatorPeer{recoveryPeer: recoveryPeer{identity: current}, log: filepath.Join(t.TempDir(), "connection.jsonl"), t: t}
			if mode == "focused" {
				p.blocked = 3
			}
			d, err := New(p.log, p, old)
			if err != nil {
				t.Fatal(err)
			}
			d.State.Bound = true
			d.State.Reload = &ReloadAttempt{Request: "test-reload", From: old.Runtime, Phase: "intent"}
			if mode != "new" && mode != "focused" {
				d.State.Input = &InputAttempt{ID: strings.Repeat("3", 32), Exchange: "reload:test-reload", Runtime: old.Runtime, Kind: "reload"}
				if mode == "submitted" {
					d.State.Input.Outcome = &InputOutcome{Disposition: "submitted", MessagesQueued: 2}
				}
				if mode == "zero" {
					d.State.Input.Outcome = &InputOutcome{Disposition: "not_sent", Retryable: true}
				}
			}
			if err = d.Save(context.Background(), "reload_intent"); err != nil {
				t.Fatal(err)
			}
			d, err = Load(p.log, p)
			if err != nil {
				t.Fatal(err)
			}
			ctx, cancel := context.WithTimeout(context.Background(), time.Second*5)
			defer cancel()
			if err = d.Continue(ctx); err != nil {
				t.Fatal(err)
			}
			want := "invoke"
			if mode == "new" || mode == "zero" {
				want = "reload,invoke"
			}
			if mode == "focused" {
				want = "escape,escape,escape,reload,invoke"
			}
			if strings.Join(p.kinds, ",") != want || !d.State.Bound || d.State.Reload.Phase != "complete" || d.State.Identity.Runtime != current.Runtime {
				t.Fatalf("%v %+v", p.kinds, d.State)
			}
			if err = d.Continue(ctx); err != nil || strings.Join(p.kinds, ",") != want {
				t.Fatalf("repeat introduced input: %v %v", err, p.kinds)
			}
		})
	}
}
func TestKnownInputOutcomeSurvivesCancellation(t *testing.T) {
	i := Identity{Runtime: strings.Repeat("1", 32), NextSlot: 1, Slots: 64, GUID: "g", Character: "c", Realm: "r", Build: "b", Product: "retail", Release: "2.5.1"}
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	p := &coordinatorPeer{log: filepath.Join(t.TempDir(), "connection.jsonl"), t: t, cancel: cancel}
	d, err := New(p.log, p, i)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = d.perform(ctx, "reload:test", i.Runtime, InputAction{Kind: "reload"}); err != nil {
		t.Fatal(err)
	}
	restored, err := Load(p.log, p)
	if err != nil || restored.State.Input.Outcome == nil || restored.State.Input.Outcome.Disposition != "submitted" {
		t.Fatalf("known fact lost: %v", err)
	}
}
