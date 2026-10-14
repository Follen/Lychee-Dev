package channel

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"hash/adler32"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
)

type pendingBindRecoveryPeer struct {
	*stopReconcilePeer
	retired []bridge.SlotEnvelope
}

func (p *pendingBindRecoveryPeer) Supersede(_ context.Context, e bridge.SlotEnvelope, _ Identity) error {
	p.retired = append(p.retired, e)
	return nil
}

func TestPendingBindRecoversThroughRuntimeCandidateWithoutReplay(t *testing.T) {
	for _, closing := range []bool{false, true} {
		name := "connect"
		if closing {
			name = "close"
		}
		t.Run(name, func(t *testing.T) {
			ctx := context.Background()
			d, old := stopReconcileFixture(t)
			p := &pendingBindRecoveryPeer{stopReconcilePeer: old}
			d.Backend, d.State.Bound = p, false
			if err := d.begin(ctx, "bind", d.State.Owner, nil); err != nil {
				t.Fatal(err)
			}
			original := d.State.Transaction.Envelope
			if err := p.Publish(ctx, original); err != nil {
				t.Fatal(err)
			}
			d.State.Transaction.Phase = "published"
			if err := d.invoke(ctx, d.State.Transaction); err != nil {
				t.Fatal(err)
			}
			// An unknown outcome still cannot authorize the original bind again.
			d.State.Input.Outcome = nil
			if err := d.Save(ctx, "unknown_bind_fixture"); err != nil {
				t.Fatal(err)
			}
			if closing {
				if err := d.RequestClose(ctx); err != nil {
					t.Fatal(err)
				}
			}
			deadline, id := d.State.ConnectBudget.DeadlineMS, d.State.ID
			p.current.Runtime, p.current.NextSlot = strings.Repeat("2", 32), 1
			loaded, err := Load(d.Log, p)
			if err != nil {
				t.Fatal(err)
			}
			d = loaded
			if err := stopReconcileContinueFor(d, 5*time.Second); err != nil {
				t.Fatal(err)
			}
			d, err = Load(d.Log, p)
			if err != nil {
				t.Fatal(err)
			}
			originalInputs, originalPublications := 0, 0
			for _, a := range p.inputs {
				if a.Envelope.Nonce == original.Nonce {
					originalInputs++
				}
			}
			for _, e := range p.publications {
				if e.Nonce == original.Nonce {
					originalPublications++
				}
			}
			if originalInputs != 1 || originalPublications != 1 || len(p.retired) != 1 || p.retired[0] != original || p.discoveries != 1 || d.State.ConnectBudget.DeadlineMS != deadline || d.State.ID != id {
				t.Fatalf("recovery replaced or replayed original exchange: state=%+v peer=%+v", d.State, p)
			}
			if d.State.Identity.Runtime != p.current.Runtime || d.State.Transaction != nil || d.State.Bound == closing || d.State.Closed != closing || d.State.Recovery == nil || d.State.Recovery.Phase != "complete" || d.State.Recovery.Transaction.Envelope != original {
				t.Fatalf("current runtime recovery did not complete: %+v", d.State)
			}
		})
	}
}

func TestRuntimeRecoveryPreservesReportAfterArtifactWriteFailure(t *testing.T) {
	ctx := context.Background()
	old := Identity{Schema: IdentitySchema, Runtime: strings.Repeat("1", 32), NextSlot: 5, Slots: bridge.SlotCount, GUID: "g", Character: "c", Realm: "r", Build: "b", Product: "retail", Release: "3.1.2"}
	current := old
	current.Runtime, current.NextSlot = strings.Repeat("2", 32), 1
	peer := &recoveryPeer{identity: current}
	d, err := New(filepath.Join(t.TempDir(), "connections", "test.jsonl"), peer, old)
	if err != nil {
		t.Fatal(err)
	}
	d.State.Bound = true
	if err := d.PrepareRequest(ctx, "preserve-report", "return 1", 5, "opaque"); err != nil {
		t.Fatal(err)
	}
	op := d.State.Operation
	op.Stage, op.PreparedNonce, op.Challenge = "release_ready", strings.Repeat("3", 32), strings.Repeat("4", 32)
	op.Result = []byte("{\"ok\":true, \"result\":\"<exact>\"}\r\n")
	op.ReportBytes, op.ReportChecksum = uint32(len(op.Result)), adler32.Checksum(op.Result)
	if err := d.begin(ctx, "release", op.Ticket, func(e *bridge.SlotEnvelope) { e.ReportBytes, e.ReportChecksum = op.ReportBytes, op.ReportChecksum }); err != nil {
		t.Fatal(err)
	}
	original := *op
	envelope := d.State.Transaction.Envelope
	output := filepath.Join(d.ResultDir, op.ID+".json")
	if err := os.MkdirAll(output, 0700); err != nil {
		t.Fatal(err)
	}
	sentinel := filepath.Join(output, "unrelated.txt")
	if err := os.WriteFile(sentinel, []byte("do not replace"), 0600); err != nil {
		t.Fatal(err)
	}
	if err := d.RecoverRuntime(ctx, current); err == nil || !strings.Contains(err.Error(), "vault.atomic_target_not_regular") {
		t.Fatalf("missing artifact-write refusal: %v", err)
	}
	d, err = Load(d.Log, peer)
	if err != nil {
		t.Fatal(err)
	}
	got := d.State.Operation
	if got.Stage != "release_ready" || got.CleanupMethod != "" || got.Ticket != original.Ticket || got.Attempt != original.Attempt || !bytes.Equal(got.Result, original.Result) || got.RecoveryBudget.DeadlineMS != original.RecoveryBudget.DeadlineMS || d.State.Recovery.Phase != "bound" || d.State.Recovery.Transaction.Envelope != envelope || peer.sends != 1 {
		t.Fatalf("failed artifact write lost pending cleanup or verified result: %+v", d.State)
	}
	if raw, err := os.ReadFile(sentinel); err != nil || string(raw) != "do not replace" {
		t.Fatalf("artifact failure damaged existing path: %q %v", raw, err)
	}
	if err := os.Remove(sentinel); err != nil {
		t.Fatal(err)
	}
	if err := os.Remove(output); err != nil {
		t.Fatal(err)
	}
	if err := d.RecoverRuntime(ctx, current); err != nil {
		t.Fatal(err)
	}
	d, err = Load(d.Log, peer)
	if err != nil {
		t.Fatal(err)
	}
	got = d.State.Operation
	if got.Stage != "complete" || got.CleanupMethod != "runtime_destroyed" || got.ID != original.ID || got.Request != original.Request || got.Ticket != original.Ticket || got.Attempt != original.Attempt || !bytes.Equal(got.Result, original.Result) || got.RecoveryBudget.DeadlineMS != original.RecoveryBudget.DeadlineMS || peer.sends != 1 {
		t.Fatalf("retry replayed business or lost report: %+v", d.State)
	}
	raw, err := os.ReadFile(output)
	if err != nil {
		t.Fatal(err)
	}
	var persisted Operation
	if err := json.Unmarshal(raw, &persisted); err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(persisted.Result, original.Result) || persisted.Ticket != original.Ticket || persisted.ReportChecksum != original.ReportChecksum {
		t.Fatalf("standalone result mismatch: %+v", persisted)
	}
}

// Acknowledging the game receipt and retiring its disk reservation are separate
// boundaries. Cancellation can persist a blocker between them; a later log
// write failure can instead require idempotently retiring the same reservation.
type receiptRetirementPeer struct {
	pendingBackend
	t            *testing.T
	log          string
	cancel       context.CancelFunc
	blockJournal bool
	consumeCalls []bridge.SlotEnvelope
	boundary     error
	receipt      Receipt
}

func (p *receiptRetirementPeer) Publish(context.Context, bridge.SlotEnvelope) error {
	p.publishes++
	return p.boundary // Stop before input for any subsequent transaction.
}
func (p *receiptRetirementPeer) ObserveInput(ctx context.Context, e bridge.SlotEnvelope, after int64, capability string) (InputObservation, error) {
	if e.Action == "reload" {
		return InputObservation{}, p.boundary
	}
	return p.pendingBackend.ObserveInput(ctx, e, after, capability)
}
func (p *receiptRetirementPeer) Observe(ctx context.Context, q ObservationQuery) (Observation, error) {
	if q.Kind == "result" {
		return Observation{}, p.boundary
	}
	e := q.Envelope
	r := p.receipt
	r.Identity = q.Identity
	r.Schema, r.Owner, r.Fence, r.NextSlot = e.Schema, e.Owner, e.Fence, e.Index+1
	r.Nonce, r.Ticket, r.Action = e.Nonce, e.Ticket, e.Action
	nonce, _ := tokenBytes(r.Nonce)
	runtime, _ := tokenBytes(r.Runtime)
	ticket, _ := tokenBytes(r.Ticket)
	kind := bridge.MemoryReceipt
	if e.Action == "confirm" {
		kind = bridge.MemoryConfirmation
	}
	record := publicationTestRecord(p.t, r, bridge.MemoryHeader{Kind: kind, State: 1, Sequence: 1, Nonce: nonce, Runtime: runtime, Ticket: ticket})
	return ObserveRecords(ctx, &publicationTestReader{record: record}, q)
}
func (p *receiptRetirementPeer) Consumed(ctx context.Context, e bridge.SlotEnvelope) error {
	p.consumeCalls = append(p.consumeCalls, e)
	if p.cancel != nil {
		p.cancel()
		p.cancel = nil
		return ctx.Err()
	}
	if p.blockJournal {
		p.blockJournal = false
		if err := os.Rename(p.log, p.log+".before-failure"); err != nil {
			p.t.Fatal(err)
		}
		if err := os.Mkdir(p.log, 0700); err != nil {
			p.t.Fatal(err)
		}
	}
	return nil
}

func TestReceiptRetirementFailureResumesWithoutReplay(t *testing.T) {
	for _, action := range []string{"bind", "prepare", "commit", "confirm", "release", "unbind", "prepare_reported", "prepare_capacity", "release_resources_pending"} {
		for _, failure := range []string{"cancel_before_retirement", "journal_after_retirement"} {
			t.Run(action+"/"+failure, func(t *testing.T) {
				base := context.Background()
				d, _ := stopReconcileFixture(t)
				peer := &receiptRetirementPeer{t: t, log: d.Log, boundary: errors.New("fixture next exchange reached")}
				d.Backend = peer
				wireAction := strings.Split(action, "_")[0]
				ticket := d.State.Owner
				if action == "bind" {
					d.State.Bound = false
				}
				if action == "unbind" {
					if err := d.RequestClose(base); err != nil {
						t.Fatal(err)
					}
				}
				if action != "bind" && action != "unbind" {
					if err := d.PrepareRequest(base, "non-idempotent", "counter = counter + 1", 5, "opaque"); err != nil {
						t.Fatal(err)
					}
					op := d.State.Operation
					ticket = op.Ticket
					if wireAction != "prepare" {
						op.PreparedNonce, op.Challenge = strings.Repeat("3", 32), strings.Repeat("4", 32)
					}
					switch wireAction {
					case "prepare":
						op.Stage = "prepared"
					case "commit":
						op.Stage = "commit_ready"
					case "confirm", "release":
						op.Stage = "confirm_ready"
						if wireAction == "release" {
							op.Stage = "release_ready"
						}
						op.Result = []byte(`{"ok":true,"result":"exact"}`)
						op.ReportBytes, op.ReportChecksum = uint32(len(op.Result)), adler32.Checksum(op.Result)
					}
					peer.receipt.PreparedNonce, peer.receipt.ReportBytes, peer.receipt.ReportChecksum = op.PreparedNonce, op.ReportBytes, op.ReportChecksum
				}
				switch action {
				case "bind":
					peer.receipt.State = "bound"
				case "prepare":
					peer.receipt.State, peer.receipt.Challenge = "prepared", strings.Repeat("4", 32)
				case "prepare_reported":
					peer.receipt.State, peer.receipt.Challenge = "reported", strings.Repeat("4", 32)
				case "commit":
					peer.receipt.State = "accepted"
				case "confirm":
					peer.receipt.State = "reported"
				case "release":
					peer.receipt.State = "released"
				case "unbind":
					peer.receipt.State = "unbound"
				case "prepare_capacity":
					peer.receipt.State, peer.receipt.Reason = "rejected", "slot_capacity"
				case "release_resources_pending":
					peer.receipt.State, peer.receipt.Reason = "rejected", "slot_resources_pending"
				}
				if err := d.begin(base, wireAction, ticket, func(e *bridge.SlotEnvelope) {
					if op := d.State.Operation; op != nil {
						switch wireAction {
						case "prepare":
							e.Code, e.CodeBytes, e.CodeChecksum, e.Budget = op.Code, len(op.Code), adler32.Checksum([]byte(op.Code)), op.Budget
						case "commit":
							e.PreparedNonce, e.Challenge = op.PreparedNonce, op.Challenge
						case "release":
							e.ReportBytes, e.ReportChecksum = op.ReportBytes, op.ReportChecksum
						}
					}
				}); err != nil {
					t.Fatal(err)
				}
				d.State.Transaction.Phase = "published"
				if err := d.invoke(base, d.State.Transaction); err != nil {
					t.Fatal(err)
				}
				original := d.State.Transaction.Envelope
				wasBound, wasStage := d.State.Bound, ""
				var deadline int64
				if op := d.State.Operation; op != nil {
					wasStage, deadline = op.Stage, op.RecoveryBudget.DeadlineMS
				}
				ctx, cancel := context.WithTimeout(base, 5*time.Second)
				defer cancel()
				if failure == "cancel_before_retirement" {
					peer.cancel = cancel
				} else {
					peer.blockJournal = true
				}
				err := d.Continue(ctx)
				if failure == "cancel_before_retirement" {
					if !errors.Is(err, context.Canceled) || !errors.Is(err, ErrPending) {
						t.Fatalf("missing retirement cancellation: %v", err)
					}
				} else {
					if err == nil {
						t.Fatal("journal failure was ignored")
					}
					if err := os.Remove(d.Log); err != nil {
						t.Fatal(err)
					}
					if err := os.Rename(d.Log+".before-failure", d.Log); err != nil {
						t.Fatal(err)
					}
				}
				d, err = Load(d.Log, peer)
				if err != nil {
					t.Fatal(err)
				}
				if d.State.Transaction == nil || d.State.Transaction.Envelope != original || d.State.Transaction.Receipt == nil || d.State.Bound != wasBound || d.State.Reload != nil {
					t.Fatalf("failed retirement advanced or lost original exchange: %+v", d.State)
				}
				if op := d.State.Operation; op != nil {
					if op.Stage != wasStage || op.CleanupMethod != "" || op.Ticket != ticket || op.RecoveryBudget.DeadlineMS != deadline {
						t.Fatalf("failed retirement changed logical operation: %+v", op)
					}
				}
				resumed, cancelResume := context.WithTimeout(base, 5*time.Second)
				defer cancelResume()
				err = d.Continue(resumed)
				terminal := action == "bind" || action == "release" || action == "unbind"
				if terminal && err != nil || !terminal && !errors.Is(err, peer.boundary) {
					t.Fatalf("resume did not advance beyond original receipt: %v", err)
				}
				if len(peer.consumeCalls) != 2 || peer.consumeCalls[0] != original || peer.consumeCalls[1] != original || peer.sends != 1 {
					t.Fatalf("original input or retirement was replayed/skipped: calls=%+v sends=%d", peer.consumeCalls, peer.sends)
				}
				if tx := d.State.Transaction; tx != nil && tx.Envelope.Nonce == original.Nonce {
					t.Fatalf("retired original transaction still pending: %+v", tx)
				}
				if action == "bind" && !d.State.Bound || action == "unbind" && (!d.State.Closed || d.State.Bound) || action == "release" && (d.State.Operation.Stage != "complete" || d.State.Operation.CleanupMethod != "released") {
					t.Fatalf("successful transition missing: %+v", d.State)
				}
				if op := d.State.Operation; op != nil && (op.Ticket != ticket || op.RecoveryBudget.DeadlineMS != deadline) {
					t.Fatalf("retry replaced operation identity or deadline: %+v", op)
				}
			})
		}
	}
}
