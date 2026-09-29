package channel

import (
	"context"
	"errors"
	"hash/adler32"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
)

type reloadRecoveryPeer struct {
	recoveryPeer
	actions []string
}

func (p *reloadRecoveryPeer) Publish(ctx context.Context, e bridge.SlotEnvelope) error {
	p.actions = append(p.actions, e.Action)
	if e.Action == "unbind" {
		p.envelope = e
		return nil
	}
	return p.recoveryPeer.Publish(ctx, e)
}
func (p *reloadRecoveryPeer) Observe(ctx context.Context, q ObservationQuery) (Observation, error) {
	if q.Envelope.Action == "unbind" {
		i := p.identity
		i.NextSlot = q.Envelope.Index + 1
		return Observation{Receipt: Receipt{Identity: i, Nonce: q.Envelope.Nonce, Ticket: q.Envelope.Ticket, Action: "unbind", State: "unbound"}}, nil
	}
	return p.recoveryPeer.Observe(ctx, q)
}

func TestExplicitReloadCanRecoverUnconfirmedReport(t *testing.T) {
	// Closing now uses the destruction-only fault cases in closing_reload_windows_test.go.
	for _, policy := range []string{"opaque", "observation"} {
		t.Run(policy+"/open", func(t *testing.T) {
			old := Identity{Runtime: strings.Repeat("1", 32), NextSlot: 4, Slots: 64, GUID: "g", Character: "c", Realm: "r", Build: "b", Product: "retail", Release: "3.0.0"}
			current := old
			current.Runtime = strings.Repeat("2", 32)
			current.NextSlot = 1
			peer := &reloadRecoveryPeer{recoveryPeer: recoveryPeer{identity: current, retireFail: true}}
			d, err := New(filepath.Join(t.TempDir(), "connections", "test.jsonl"), peer, old)
			if err != nil {
				t.Fatal(err)
			}
			d.State.Bound = true
			ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
			defer cancel()
			if err = d.PrepareRequest(ctx, "settings", "return 1", 30, policy); err != nil {
				t.Fatal(err)
			}
			op := d.State.Operation
			op.Stage = "confirm_ready"
			op.PreparedNonce = strings.Repeat("3", 32)
			op.Challenge = strings.Repeat("4", 32)
			op.Result = []byte(`{"ok":true,"resourcesReleased":true}`)
			op.ReportBytes = uint32(len(op.Result))
			op.ReportChecksum = adler32.Checksum(op.Result)
			if err = d.begin(ctx, "confirm", op.Ticket, nil); err != nil {
				t.Fatal(err)
			}
			d.State.Transaction.Phase = "input_attempted"
			if err = d.Save(ctx, "confirm_missing"); err != nil {
				t.Fatal(err)
			}
			if err = d.RequestReload(ctx, "recover-settings"); err != nil {
				t.Fatalf("explicit recovery reload rejected while confirm is pending: %v", err)
			}
			if err = d.Continue(ctx); err == nil {
				t.Fatal("missing retirement fault")
			}
			d, err = Load(d.Log, peer)
			if err != nil {
				t.Fatal(err)
			}
			if err = d.Continue(ctx); err != nil {
				t.Fatal(err)
			}
			d, err = Load(d.Log, peer)
			if err != nil {
				t.Fatal(err)
			}
			stage, attempt := "execution_unknown", 1
			if policy == "observation" {
				stage, attempt = "prepared", 2
			}
			if d.State.Reload.Phase != "complete" || !d.State.Bound || d.State.Closed || d.State.Transaction != nil || d.State.Operation.Stage != stage || d.State.Operation.Attempt != attempt || d.State.Recovery.Transaction.Envelope.Action != "confirm" {
				t.Fatalf("lost uncertain operation or recovery: %+v", d.State)
			}
			if stage == "execution_unknown" && string(d.State.Operation.Result) != string(op.Result) {
				t.Fatal("lost candidate evidence")
			}
			wantInputs := 2
			if peer.sends != wantInputs {
				t.Fatalf("want reload and fresh bind only, got %d inputs", peer.sends)
			}
			if policy == "opaque" {
				if err = d.Continue(ctx); !errors.Is(err, ErrExecutionUnknown) || peer.sends != wantInputs {
					t.Fatal("unknown operation replayed", err)
				}
			}
		})
	}
}
