package channel

import (
	"context"
	"errors"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
)

type slotReloadPeer struct {
	recoveryPeer
	flags        map[string]bool
	inputs       []string
	publications []string
	retired      []bridge.SlotEnvelope
	blocked      bool
	uncertain    bool
}

func withSlotReloadFlag(s InputObservation, required bool) InputObservation {
	s.SlotReloadRequired = required
	return s
}

func (p *slotReloadPeer) ObserveInput(ctx context.Context, e bridge.SlotEnvelope, after int64, capability string) (InputObservation, error) {
	s, err := p.recoveryPeer.ObserveInput(ctx, e, after, capability)
	if p.blocked {
		blocked := true
		s.InputBlocked, s.Reason = &blocked, "combat"
	}
	return withSlotReloadFlag(s, p.flags[e.Runtime]), err
}

func (p *slotReloadPeer) Publish(ctx context.Context, e bridge.SlotEnvelope) error {
	p.publications = append(p.publications, e.Runtime)
	return p.recoveryPeer.Publish(ctx, e)
}

func (p *slotReloadPeer) Input(_ context.Context, a InputAction) (InputOutcome, error) {
	p.inputs = append(p.inputs, a.Kind+":"+a.Envelope.Runtime)
	if p.uncertain {
		return InputOutcome{Disposition: "uncertain", MessagesQueued: 1, AtMillis: 1100}, errors.New("injected unknown reload")
	}
	if a.Kind == "reload" {
		p.identity.Runtime = strings.Repeat(string(a.Envelope.Runtime[0]+1), 32)
		p.identity.Owner, p.identity.Fence, p.identity.NextSlot = "", 0, 1
	}
	return InputOutcome{Disposition: "submitted", MessagesQueued: 1, AtMillis: 1100}, nil
}

func (p *slotReloadPeer) Supersede(_ context.Context, old bridge.SlotEnvelope, _ Identity) error {
	p.retired = append(p.retired, old)
	return nil
}

func newSlotReloadDriver(t *testing.T, flags ...string) (*Driver, *slotReloadPeer) {
	t.Helper()
	i := Identity{Schema: IdentitySchema, InputState: InputSchema, Runtime: strings.Repeat("1", 32), NextSlot: 1, Slots: 500, GUID: "g", Character: "c", Realm: "r", Build: "b", Product: "retail", Release: "3.2.1"}
	p := &slotReloadPeer{recoveryPeer: recoveryPeer{identity: i}, flags: make(map[string]bool)}
	for _, runtime := range flags {
		p.flags[strings.Repeat(runtime, 32)] = true
	}
	d, err := New(filepath.Join(t.TempDir(), "connections", "test.jsonl"), p, i)
	if err != nil {
		t.Fatal(err)
	}
	return d, p
}

func TestStartupSlotsReloadBeforeBind(t *testing.T) {
	for _, required := range []bool{false, true} {
		t.Run(map[bool]string{false: "already_enabled", true: "enabled_at_startup"}[required], func(t *testing.T) {
			d, p := newSlotReloadDriver(t)
			old, fresh := strings.Repeat("1", 32), strings.Repeat("2", 32)
			p.flags[old] = required
			id, deadline := d.State.ID, d.State.ConnectBudget.DeadlineMS
			ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
			defer cancel()
			if err := d.Continue(ctx); err != nil {
				t.Fatal(err)
			}
			wantInputs, wantRuntime := []string{"invoke:" + old}, old
			if required {
				wantInputs, wantRuntime = []string{"reload:" + old, "invoke:" + fresh}, fresh
				if d.State.Reload == nil || !d.State.Reload.Automatic || d.State.Reload.Phase != "complete" || d.State.Reload.RecoveryBudget.DeadlineMS != deadline {
					t.Fatalf("lost startup reload or original deadline: %+v", d.State.Reload)
				}
			} else if d.State.Reload != nil {
				t.Fatal("enabled slots caused a reload")
			}
			if !reflect.DeepEqual(p.inputs, wantInputs) || !reflect.DeepEqual(p.publications, []string{wantRuntime}) || !d.State.Bound || d.State.ID != id {
				t.Fatalf("bound before slot activation or changed connection: inputs=%v publications=%v state=%+v", p.inputs, p.publications, d.State)
			}
		})
	}
}

func TestStartupSlotsContinueOriginalExplicitRecovery(t *testing.T) {
	for _, policy := range []string{"opaque", "observation"} {
		t.Run(policy, func(t *testing.T) {
			d, p := newSlotReloadDriver(t, "2")
			d.State.Bound = true
			ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
			defer cancel()
			if err := d.PrepareRequest(ctx, "original-business", "return 1", 5, policy); err != nil {
				t.Fatal(err)
			}
			op := d.State.Operation
			op.Stage = "running"
			op.PreparedNonce, op.Challenge = strings.Repeat("a", 32), strings.Repeat("b", 32)
			if err := d.begin(ctx, "confirm", op.Ticket, nil); err != nil {
				t.Fatal(err)
			}
			d.State.Transaction.Phase = "input_attempted"
			original := d.State.Transaction.Envelope
			d.State.Input = &InputAttempt{ID: strings.Repeat("c", 32), Exchange: original.Nonce, Runtime: original.Runtime, Kind: "invoke"}
			if err := d.RequestReload(ctx, "original-reload"); err != nil {
				t.Fatal(err)
			}
			deadline := d.State.Reload.RecoveryBudget.DeadlineMS
			if err := d.Continue(ctx); err != nil {
				t.Fatal(err)
			}
			loaded, err := Load(d.Log, p)
			if err != nil {
				t.Fatal(err)
			}
			r := loaded.State.Reload
			wantInputs := []string{"reload:" + strings.Repeat("1", 32), "reload:" + strings.Repeat("2", 32), "invoke:" + strings.Repeat("3", 32)}
			if r.Request != "original-reload" || r.Automatic || r.Phase != "complete" || r.RecoveryBudget.DeadlineMS != deadline || !reflect.DeepEqual(p.inputs, wantInputs) || !reflect.DeepEqual(p.publications, []string{strings.Repeat("3", 32)}) {
				t.Fatalf("reload continuation lost original intent: reload=%+v inputs=%v publications=%v", r, p.inputs, p.publications)
			}
			if len(p.retired) != 1 || p.retired[0].Nonce != original.Nonce || loaded.State.Recovery.Transaction.Envelope.Nonce != original.Nonce || loaded.State.Operation.ID != op.ID || loaded.State.Operation.Request != op.Request {
				t.Fatal("lost original pending exchange or business identity")
			}
			wantStage, wantAttempt := "execution_unknown", 1
			if policy == "observation" {
				wantStage, wantAttempt = "prepared", 2
			}
			if loaded.State.Operation.Stage != wantStage || loaded.State.Operation.Attempt != wantAttempt {
				t.Fatalf("changed replay policy: %+v", loaded.State.Operation)
			}
		})
	}
}

func TestStartupSlotsSubmittedOrUnknownReloadIsNotReplayed(t *testing.T) {
	for _, disposition := range []string{"submitted", "uncertain", "missing"} {
		t.Run(disposition, func(t *testing.T) {
			d, p := newSlotReloadDriver(t, "1")
			p.uncertain = true
			ctx := context.Background()
			if err := d.next(ctx); !errors.Is(err, errProgress) {
				t.Fatalf("startup did not schedule reload: %v", err)
			}
			deadline := d.State.Reload.RecoveryBudget.DeadlineMS
			if err := d.next(ctx); err == nil || d.State.Input.Outcome.Disposition != "uncertain" {
				t.Fatal("missing injected uncertain outcome", err)
			}
			if disposition == "missing" {
				d.State.Input.Outcome = nil
			} else {
				d.State.Input.Outcome.Disposition = disposition
			}
			// Crash after physical input, before the reload phase is advanced.
			if err := d.Save(ctx, "reload_outcome_fixture"); err != nil {
				t.Fatal(err)
			}
			for retry := 0; retry < 2; retry++ {
				var err error
				d, err = Load(d.Log, p)
				if err != nil {
					t.Fatal(err)
				}
				err = d.next(ctx)
				if !errors.Is(err, errProgress) && !errors.Is(err, ErrPending) {
					t.Fatal(err)
				}
				if len(p.inputs) != 1 || len(p.publications) != 0 || d.State.Reload.RecoveryBudget.DeadlineMS != deadline {
					t.Fatal("resumed reload was replayed or renewed")
				}
			}
		})
	}
}

func TestStartupSlotsBlockedInputWaitsWithinOriginalBudget(t *testing.T) {
	d, p := newSlotReloadDriver(t, "1")
	p.blocked = true
	deadline := d.State.ConnectBudget.DeadlineMS
	ctx, cancel := context.WithTimeout(context.Background(), 100*time.Millisecond)
	err := d.Continue(ctx)
	cancel()
	if !errors.Is(err, ErrPending) || len(p.inputs) != 0 || d.Waiting != "combat" {
		t.Fatalf("blocked startup sent input: %v %+v", err, p.inputs)
	}
	d, err = Load(d.Log, p)
	if err != nil {
		t.Fatal(err)
	}
	p.blocked = false
	ctx, cancel = context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	if err = d.Continue(ctx); err != nil {
		t.Fatal(err)
	}
	if d.State.Reload == nil || d.State.Reload.RecoveryBudget.DeadlineMS != deadline || len(p.inputs) != 2 || p.inputs[0] != "reload:"+strings.Repeat("1", 32) {
		t.Fatal("resume lost slot reload or renewed the deadline")
	}
}

func TestStartupSlotsDoNotReplaceUnknownInvoke(t *testing.T) {
	d, p := newSlotReloadDriver(t, "1")
	e := bridge.SlotEnvelope{Runtime: d.State.Identity.Runtime, Nonce: strings.Repeat("d", 32), Action: "bind"}
	d.State.Input = &InputAttempt{Exchange: e.Nonce, Runtime: e.Runtime, Kind: "invoke"}
	if err := d.submitInput(context.Background(), e.Nonce, e.Runtime, "invoke", e); err != nil || len(p.inputs) != 0 || d.State.Reload != nil {
		t.Fatalf("slot flag bypassed unknown invoke: %v", err)
	}
}

func TestStartupSlotFlagIsRevalidatedBeforeNativeInput(t *testing.T) {
	ready := false
	s := InputObservation{InputBlocked: &ready, Bindings: primaryInputBindings()}
	if sameInputProfile(s, withSlotReloadFlag(s, true)) {
		t.Fatal("changed slot activation request retained input authority")
	}
}
