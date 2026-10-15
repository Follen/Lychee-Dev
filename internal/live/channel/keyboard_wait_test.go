package channel

import (
	"context"
	"errors"
	"testing"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/desktop"
)

type keyboardCadencePeer struct {
	*cadencePeer
	busy     bool
	attempts int
	receipts []bridge.SlotEnvelope
}

func (p *keyboardCadencePeer) Input(ctx context.Context, a InputAction) (InputOutcome, error) {
	p.attempts++
	if p.busy {
		return InputOutcome{Disposition: "not_sent", AtMillis: p.now, Retryable: true, Reason: desktop.ErrKeyboardActive.Error()}, desktop.ErrKeyboardActive
	}
	return p.cadencePeer.Input(ctx, a)
}

func (p *keyboardCadencePeer) Observe(ctx context.Context, q ObservationQuery) (Observation, error) {
	if q.Kind == "receipt" {
		p.receipts = append(p.receipts, q.Envelope)
	}
	return p.cadencePeer.Observe(ctx, q)
}

func TestKeyboardWaitRechecksInputForTheOriginalExchange(t *testing.T) {
	ctx := context.Background()
	d, base := cadenceFixture(t)
	p := &keyboardCadencePeer{cadencePeer: base, busy: true}
	d.Backend = p
	if err := d.Connect(ctx); !errors.Is(err, ErrPending) || !errors.Is(err, desktop.ErrKeyboardActive) {
		t.Fatalf("busy keyboard did not wait: %v", err)
	}
	original := d.State.Transaction.Envelope
	// Binding first checks startup slot activation, then observes before input.
	if d.State.Input.Outcome.MessagesQueued != 0 || d.State.Input.Outcome.Disposition != "not_sent" || len(p.after) != 2 || len(p.inputs) != 0 {
		t.Fatal("busy attempt was not recorded as known-zero input")
	}
	p.sample, p.now, p.busy = 1500, 1500, false
	if err := d.Connect(ctx); err != nil {
		t.Fatal(err)
	}
	if len(p.after) != 3 || len(p.inputs) != 1 || p.inputs[0].Observation.SampleMillis != 1500 || p.inputs[0].Envelope != original || !d.State.Bound {
		t.Fatalf("quiet retry reused stale INPUT or replaced the exchange: %+v", p.inputs)
	}
}

func TestKeyboardActivityDoesNotDelayOrReplaySubmittedReceipt(t *testing.T) {
	for _, disposition := range []string{"submitted", "uncertain", "missing"} {
		t.Run(disposition, func(t *testing.T) {
			ctx := context.Background()
			d, base := cadenceFixture(t)
			p := &keyboardCadencePeer{cadencePeer: base}
			d.Backend = p
			if err := d.begin(ctx, "bind", d.State.Owner, nil); err != nil {
				t.Fatal(err)
			}
			tx := d.State.Transaction
			if err := p.Publish(ctx, tx.Envelope); err != nil {
				t.Fatal(err)
			}
			tx.Phase = "published"
			if err := d.invoke(ctx, tx); err != nil {
				t.Fatal(err)
			}
			if disposition == "missing" {
				d.State.Input.Outcome = nil
				tx.Phase = "published" // crash after submission, before recording its result
			} else {
				d.State.Input.Outcome.Disposition = disposition
			}
			if err := d.Save(ctx, "keyboard_recovery_fixture"); err != nil {
				t.Fatal(err)
			}
			originalInput, originalEnvelope := d.State.Input.ID, tx.Envelope
			p.busy = true
			loaded, err := Load(d.Log, p)
			if err != nil {
				t.Fatal(err)
			}
			if err := loaded.Connect(ctx); err != nil {
				t.Fatal("keyboard activity blocked the original receipt", err)
			}
			if p.attempts != 1 || len(p.after) != 1 || len(p.inputs) != 1 || len(p.receipts) != 1 || p.receipts[0] != originalEnvelope || loaded.State.Input.ID != originalInput || !loaded.State.Bound {
				t.Fatal("recovery observed new INPUT or replayed a submitted/uncertain request")
			}
		})
	}
}
