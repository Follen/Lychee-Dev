//go:build windows && amd64

package channel

import (
	"context"
	"errors"
	"fmt"
	"hash/adler32"
	"reflect"
	"strings"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/live/journal"
)

type retirementPeer struct {
	observeError error
	retireError  error
	onObserve    func()
	proof        *RuntimeReplacementProof
	observed     int
	retired      []bridge.SlotEnvelope
	fail         bool
	calls        int
	failAt       int
	afterRetire  func()
	applied      map[string]bridge.SlotEnvelope
}

func (p *retirementPeer) ObserveRuntimeReplacement(context.Context, Identity) (*RuntimeReplacementProof, error) {
	p.observed++
	if p.onObserve != nil {
		p.onObserve()
	}
	return p.proof, p.observeError
}
func (p *retirementPeer) RetireReplacedRuntime(_ context.Context, e bridge.SlotEnvelope, _ *RuntimeReplacementProof) error {
	p.calls++
	if p.retireError != nil {
		return p.retireError
	}
	if p.fail || p.calls == p.failAt {
		return errors.New("injected retirement failure")
	}
	p.retired = append(p.retired, e)
	if p.applied == nil {
		p.applied = map[string]bridge.SlotEnvelope{}
	}
	if previous, exists := p.applied[e.Nonce]; exists && previous != e {
		return errors.New("retirement changed original envelope")
	}
	p.applied[e.Nonce] = e
	if p.afterRetire != nil {
		p.afterRetire()
	}
	return nil
}

func retirementFixture(t *testing.T) (*Driver, *stopReconcilePeer, *retirementPeer) {
	t.Helper()
	d, transport := stopReconcileFixture(t)
	d.State.Identity.InputState = bridge.InputSignalCapability
	if err := d.PrepareRequest(context.Background(), "old-request", "return 1", 5, "opaque"); err != nil {
		t.Fatal(err)
	}
	if err := d.begin(context.Background(), "bind", d.State.Owner, nil); err != nil {
		t.Fatal(err)
	}
	d.State.Transaction.Phase = "input_attempted"
	d.State.Operation.RecoveryBudget = NewDurableBudget(time.Now().Add(-time.Hour), time.Minute, false)
	d.State.Operation.RecoveryBudget.Exhausted = true
	current := d.State.Identity
	current.Runtime = strings.Repeat("0", 31) + "1" // Tokens are not a cross-character clock.
	current.GUID, current.Character = "new-guid", "new-character"
	current.Owner, current.Fence, current.NextSlot = "", 0, 1
	blocked := true
	observation := InputObservation{Schema: InputSchema, Runtime: current.Runtime, GUID: current.GUID, Build: current.Build, NextSlot: 1, InputBlocked: &blocked, Reason: "input_binding_unavailable"}
	observation.SampleMillis = 1200
	first := observation
	first.SampleMillis = 1100
	proof := &RuntimeReplacementProof{Schema: RuntimeReplacementProofSchema, ProcessID: 123, ProcessStartedAt: 456, Current: current,
		Witness: RuntimeReplacementWitness{Address: 4096, Length: 512, BeforeSHA256: strings.Repeat("1", 64), AfterSHA256: strings.Repeat("2", 64), First: first, FirstSequence: 1, Sequence: 2, Observation: observation}}
	if err := d.Save(context.Background(), "unknown_fixture"); err != nil {
		t.Fatal(err)
	}
	return d, transport, &retirementPeer{proof: proof}
}

func TestRuntimeRetirementClosesExpiredUnknownWithoutInput(t *testing.T) {
	d, transport, proof := retirementFixture(t)
	oldIdentity := d.State.Identity
	closed, err := d.closeReplacedRuntime(context.Background(), proof, retirementTarget())
	if err != nil || !closed || !d.State.Closed {
		t.Fatalf("obsolete connection retained: closed=%v err=%v", closed, err)
	}
	if d.State.Identity != oldIdentity || d.State.Bound || d.State.ProcessEnd != "" {
		t.Fatal("invented binding, actor handoff or process exit")
	}
	if d.State.Operation.Stage != "execution_unknown" || present(d).Complete || present(d).ReportState != "unavailable" {
		t.Fatal("promoted unknown business")
	}
	if len(transport.inputs) != 0 || len(transport.publications) != 0 || len(proof.retired) != 1 {
		t.Fatal("sent input or failed precise retirement")
	}
	if !d.State.Operation.RecoveryBudget.Exhausted {
		t.Fatal("renewed old goal")
	}
	loaded, err := Load(d.Log, transport)
	if err != nil || loaded.State.RuntimeEnd == nil || !loaded.State.Closed {
		t.Fatal("lost proof", err)
	}
}

func TestRuntimeRetirementResumesAfterProofWithoutNewObservation(t *testing.T) {
	d, transport, proof := retirementFixture(t)
	proof.fail = true
	if closed, err := d.closeReplacedRuntime(context.Background(), proof, retirementTarget()); err == nil || closed {
		t.Fatal("fault hidden")
	}
	events, err := journal.ReadMemoryLog(d.Log)
	if err != nil || events[len(events)-1].Kind != "runtime_replacement_observed" {
		t.Fatal("proof was not durable before retirement", err)
	}
	d, err = Load(d.Log, transport)
	if err != nil {
		t.Fatal(err)
	}
	proof.fail = false
	proof.proof = nil
	closed, err := d.closeReplacedRuntime(context.Background(), proof, retirementTarget())
	if err != nil || !closed || proof.observed != 1 {
		t.Fatal("re-observed or lost saved proof", err)
	}
}

func TestRuntimeRetirementMissingProofLeavesUnknownUntouched(t *testing.T) {
	d, transport, proof := retirementFixture(t)
	proof.proof = nil
	closed, err := d.closeReplacedRuntime(context.Background(), proof, retirementTarget())
	if err != nil || closed || d.State.Closed || d.State.Transaction == nil || len(proof.retired) != 0 || len(transport.inputs) != 0 {
		t.Fatal("retired without evidence", err)
	}
}

func retirementTarget() desktop.WindowIdentity {
	return desktop.WindowIdentity{ProcessID: 123, ProcessStartedAt: 456}
}

func TestRuntimeRetirementProofBlocksEveryOldInputEntry(t *testing.T) {
	d, transport, proof := retirementFixture(t)
	d.State.RuntimeEnd = proof.proof
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	if err := d.Continue(ctx); !errors.Is(err, ErrPending) {
		t.Fatal(err)
	}
	if err := d.RequestReload(ctx, "new-reload"); !errors.Is(err, ErrPending) {
		t.Fatal(err)
	}
	if err := d.PrepareRequest(ctx, "new-business", "return 2", 5, "opaque"); !errors.Is(err, ErrPending) {
		t.Fatal(err)
	}
	if err := d.submitInput(ctx, "retry", d.State.Identity.Runtime, "invoke", d.State.Transaction.Envelope); !errors.Is(err, ErrPending) {
		t.Fatal(err)
	}
	if _, err := d.advance(ctx); !errors.Is(err, ErrPending) {
		t.Fatal(err)
	}
	if len(transport.inputs) != 0 || len(transport.publications) != 0 || d.State.Reload != nil || d.State.Operation.Request != "old-request" {
		t.Fatal("revived retired runtime")
	}
	if c := d.continuation(); c.Kind != "continue" || c.Goal != "close" || c.RemainingBudgetMS != nil {
		t.Fatal(c)
	}
}

func TestRuntimeRetirementRejectsOtherProcessProofEvenWithoutTransactions(t *testing.T) {
	d, _, proof := retirementFixture(t)
	d.State.Transaction = nil
	proof.proof.ProcessStartedAt++
	closed, err := d.closeReplacedRuntime(context.Background(), proof, retirementTarget())
	if err == nil || closed || d.State.RuntimeEnd != nil || len(proof.retired) != 0 {
		t.Fatal("accepted other process proof")
	}
}

func retirementWithRecovery(t *testing.T) (*Driver, *stopReconcilePeer, *retirementPeer) {
	t.Helper()
	d, transport, peer := retirementFixture(t)
	from := d.State.Identity
	from.Runtime = strings.Repeat("0", 31) + "2"
	tx := *d.State.Transaction
	tx.Envelope.Runtime = from.Runtime
	tx.Envelope.Nonce = strings.Repeat("7", 32)
	tx.Envelope.Index = 2
	tx.Envelope.StartSlot = 2
	d.State.Recovery = &RuntimeRecovery{From: from, Transaction: &tx, Phase: "binding"}
	if err := d.Save(context.Background(), "two_retirements_fixture"); err != nil {
		t.Fatal(err)
	}
	return d, transport, peer
}

func TestRuntimeRetirementSecondFailureRetriesOriginalEnvelopes(t *testing.T) {
	d, transport, peer := retirementWithRecovery(t)
	first, second := d.State.Transaction.Envelope, d.State.Recovery.Transaction.Envelope
	peer.failAt = 2
	closed, err := d.closeReplacedRuntime(context.Background(), peer, retirementTarget())
	if err == nil || closed || d.State.RuntimeEnd == nil || len(peer.applied) != 1 || peer.applied[first.Nonce] != first {
		t.Fatalf("partial retirement lost: %v", err)
	}
	var loadErr error
	d, loadErr = Load(d.Log, transport)
	if loadErr != nil {
		t.Fatal(loadErr)
	}
	if d.State.Transaction.Envelope != first || d.State.Recovery.Transaction.Envelope != second {
		t.Fatal("lost exact original transactions")
	}
	peer.failAt = 0
	peer.proof = nil
	closed, err = d.closeReplacedRuntime(context.Background(), peer, retirementTarget())
	if err != nil || !closed || peer.observed != 1 || len(peer.applied) != 2 {
		t.Fatal("retry failed", err)
	}
	if !reflect.DeepEqual(peer.retired, []bridge.SlotEnvelope{first, first, second}) || peer.applied[second.Nonce] != second {
		t.Fatal("wrong retry targets", peer.retired)
	}
	observed, calls := peer.observed, peer.calls
	if closed, err = d.closeReplacedRuntime(context.Background(), peer, retirementTarget()); err != nil || !closed || peer.observed != observed || peer.calls != calls {
		t.Fatal("closed connection repeated work", err)
	}
	if len(transport.inputs) != 0 || len(transport.publications) != 0 {
		t.Fatal("retirement issued input/publication")
	}
}

func TestRuntimeRetirementTerminalSaveFailurePreservesProofAndTransactions(t *testing.T) {
	d, transport, peer := retirementWithRecovery(t)
	originalLog := d.Log
	first, second := *d.State.Transaction, *d.State.Recovery.Transaction
	originalOperation := *d.State.Operation
	blockedLog := t.TempDir() // Opening a directory as the journal is a real persistence failure.
	peer.afterRetire = func() {
		if peer.calls == 2 {
			d.Log = blockedLog
		}
	}
	closed, err := d.closeReplacedRuntime(context.Background(), peer, retirementTarget())
	d.Log = originalLog
	if err == nil || closed || d.State.Closed || d.State.RuntimeEnd == nil {
		t.Fatal("terminal persistence fault hidden", err)
	}
	if !reflect.DeepEqual(*d.State.Transaction, first) || !reflect.DeepEqual(*d.State.Recovery.Transaction, second) || !reflect.DeepEqual(*d.State.Operation, originalOperation) {
		t.Fatal("failed Save mutated original state")
	}
	events, readErr := journal.ReadMemoryLog(originalLog)
	if readErr != nil || events[len(events)-1].Kind != "runtime_replacement_observed" {
		t.Fatal("proof not preserved before failure", readErr)
	}
	d, err = Load(originalLog, transport)
	if err != nil {
		t.Fatal(err)
	}
	peer.afterRetire = nil
	peer.proof = nil
	closed, err = d.closeReplacedRuntime(context.Background(), peer, retirementTarget())
	if err != nil || !closed || peer.observed != 1 || len(peer.applied) != 2 {
		t.Fatal("failed to recover persisted proof", err)
	}
	if !reflect.DeepEqual(peer.retired, []bridge.SlotEnvelope{first.Envelope, second.Envelope, first.Envelope, second.Envelope}) {
		t.Fatal("retry did not retain exact envelopes")
	}
}

func TestRuntimeRetirementPreservesVerifiedAndUnconfirmedReports(t *testing.T) {
	for _, stage := range []string{"result_verified", "confirm_ready"} {
		t.Run(stage, func(t *testing.T) {
			d, _, peer := retirementFixture(t)
			op := d.State.Operation
			op.Stage = stage
			op.PreparedNonce = strings.Repeat("6", 32)
			op.Challenge = strings.Repeat("7", 32)
			op.Result = []byte(`{"ok":false,"value":"retained original report"}`)
			op.ReportBytes = uint32(len(op.Result))
			op.ReportChecksum = adler32.Checksum(op.Result)
			want := append([]byte(nil), op.Result...)
			if err := d.Save(context.Background(), "report_retirement_fixture"); err != nil {
				t.Fatal(err)
			}
			closed, err := d.closeReplacedRuntime(context.Background(), peer, retirementTarget())
			if err != nil || !closed {
				t.Fatal(err)
			}
			result := present(d)
			if string(d.State.Operation.Result) != string(want) {
				t.Fatal("discarded original report evidence")
			}
			if stage == "result_verified" {
				if result.ReportState != "verified" || !result.Complete || string(result.Report) != string(want) || result.CleanupMethod != "runtime_destroyed" {
					t.Fatalf("lost verified report: %+v", result)
				}
			} else if result.ReportState != "unavailable" || result.Complete || result.OperationState != "execution_unknown" || len(result.Report) != 0 {
				t.Fatalf("promoted unconfirmed report: %+v", result)
			}
		})
	}
}

func TestOptionalRetirementTimeoutContinuesHealthyDisconnect(t *testing.T) {
	d, transport := stopReconcileFixture(t)
	if err := d.RequestClose(context.Background()); err != nil {
		t.Fatal(err)
	}
	budget := *d.State.CloseBudget
	peer := &retirementPeer{observeError: errors.Join(ErrPending, context.DeadlineExceeded)}
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	err := d.continueOrRetireWith(ctx, peer, retirementTarget(), true)
	if err != nil || !d.State.Closed || peer.observed != 1 || d.State.RuntimeEnd != nil {
		t.Fatalf("optional timeout blocked normal close: %v", err)
	}
	if len(transport.publications) != 1 || transport.publications[0].Action != "unbind" || len(transport.inputs) != 1 || transport.inputs[0].Kind != "invoke" {
		t.Fatal("normal unbind did not run exactly once")
	}
	if d.State.CloseBudget.StartedAtMS != budget.StartedAtMS || d.State.CloseBudget.DeadlineMS != budget.DeadlineMS {
		t.Fatal("renewed close budget")
	}
	loaded, err := Load(d.Log, transport)
	if err != nil || !loaded.State.Closed {
		t.Fatal("close not durably complete", err)
	}
}

func TestOptionalRetirementErrorsDoNotFallBack(t *testing.T) {
	for _, kind := range []string{"parent_cancel", "parent_deadline", "non_timeout", "saved_proof"} {
		t.Run(kind, func(t *testing.T) {
			d, transport, peer := retirementFixture(t)
			ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
			defer cancel()
			expected := errors.Join(ErrPending, context.DeadlineExceeded)
			if kind == "saved_proof" {
				d.State.RuntimeEnd = peer.proof
				peer.retireError = expected
			} else {
				peer.proof = nil
				peer.observeError = expected
				switch kind {
				case "parent_cancel":
					peer.onObserve = cancel
				case "parent_deadline":
					cancel()
					ctx, cancel = context.WithDeadline(context.Background(), time.Now().Add(-time.Second))
					defer cancel()
				case "non_timeout":
					expected = errors.New("invalid observation")
					peer.observeError = expected
				}
			}
			before := *d.State.Operation.RecoveryBudget
			err := d.continueOrRetireWith(ctx, peer, retirementTarget(), true)
			if !errors.Is(err, expected) || d.State.Closed || len(transport.inputs) != 0 || len(transport.publications) != 0 {
				t.Fatal("error incorrectly fell back", err)
			}
			if *d.State.Operation.RecoveryBudget != before {
				t.Fatal("continued or renewed business budget")
			}
			if kind == "saved_proof" && (peer.observed != 0 || d.State.RuntimeEnd == nil) {
				t.Fatal("lost durable proof")
			}
		})
	}
}

func TestOptionalRetirementRawWrappedAndMixedTimeout(t *testing.T) {
	fault := errors.New("publication I/O failure")
	for _, tc := range []struct {
		name   string
		err    error
		closes bool
	}{
		{"raw", context.DeadlineExceeded, true},
		{"wrapped", fmt.Errorf("observe: %w", context.DeadlineExceeded), true},
		{"mixed", errors.Join(ErrPending, context.DeadlineExceeded, fault), false},
		{"cancelled", errors.Join(ErrPending, context.DeadlineExceeded, context.Canceled), false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			d, transport := stopReconcileFixture(t)
			if err := d.RequestClose(context.Background()); err != nil {
				t.Fatal(err)
			}
			peer := &retirementPeer{observeError: tc.err}
			ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
			defer cancel()
			err := d.continueOrRetireWith(ctx, peer, retirementTarget(), true)
			if tc.closes {
				if err != nil || !d.State.Closed || len(transport.inputs) != 1 {
					t.Fatal("optional timeout blocked close", err)
				}
			} else if !errors.Is(err, tc.err) || d.State.Closed || len(transport.inputs) != 0 || len(transport.publications) != 0 {
				t.Fatal("suppressed real fault", err)
			}
		})
	}
}
