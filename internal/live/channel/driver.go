package channel

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"hash/adler32"
	"path/filepath"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/live/journal"

	"github.com/follenfang/lycheedev/internal/vault"
)

type Transaction struct {
	Envelope         bridge.SlotEnvelope `json:"envelope"`
	Phase            string              `json:"phase"`
	Receipt          *Receipt            `json:"receipt,omitempty"`
	EscapeAttempts   int                 `json:"escapeAttempts,omitempty"`
	InputObservation *InputObservation   `json:"inputObservation,omitempty"`
}
type Operation struct {
	RecoveryBudget *DurableBudget `json:"recoveryBudget,omitempty"`
	CleanupMethod  string         `json:"cleanupMethod,omitempty"`
	Attempt        int            `json:"attempt,omitempty"`
	Request        string         `json:"request,omitempty"`
	Origin         *Identity      `json:"origin,omitempty"`
	ID             string         `json:"id"`
	Ticket         string         `json:"ticket"`
	Code           string         `json:"code"`
	Budget         int            `json:"budget"`
	Policy         string         `json:"policy"`
	Stage          string         `json:"stage"`
	PreparedNonce  string         `json:"preparedNonce,omitempty"`
	Challenge      string         `json:"challenge,omitempty"`
	ReportBytes    uint32         `json:"reportBytes,omitempty"`
	ReportChecksum uint32         `json:"reportChecksum,omitempty"`
	// Keep original payload bytes across JSON persistence: RawMessage is
	// compacted/HTML-escaped by encoding/json and can change length/checksum.
	Result []byte `json:"resultBytes,omitempty"`
}
type State struct {
	ProgressVersion uint64                   `json:"progressVersion,omitempty"`
	Blocker         *Blocker                 `json:"blocker,omitempty"`
	ConnectBudget   *DurableBudget           `json:"connectBudget,omitempty"`
	CloseBudget     *DurableBudget           `json:"closeBudget,omitempty"`
	ProcessEnd      string                   `json:"processEnd,omitempty"`
	RuntimeEnd      *RuntimeReplacementProof `json:"runtimeEnd,omitempty"`
	Artifacts       map[string]string        `json:"artifacts,omitempty"`
	Archive         string                   `json:"archive,omitempty"`
	Schema          string                   `json:"schema"`
	ID              string                   `json:"id"`
	Owner           string                   `json:"owner"`
	Identity        Identity                 `json:"identity"`
	Bound           bool                     `json:"bound"`
	Closed          bool                     `json:"closed,omitempty"`
	Transaction     *Transaction             `json:"transaction,omitempty"`
	Operation       *Operation               `json:"operation,omitempty"`
	Reload          *ReloadAttempt           `json:"reload,omitempty"`
	Recovery        *RuntimeRecovery         `json:"recovery,omitempty"`
	Input           *InputAttempt            `json:"input,omitempty"`
	Closing         bool                     `json:"closing,omitempty"`
}

type ReloadAttempt struct {
	Automatic        bool           `json:"automatic,omitempty"`
	RecoveryBudget   *DurableBudget `json:"recoveryBudget,omitempty"`
	ReconcilePending bool           `json:"reconcilePending,omitempty"`
	Request          string         `json:"request"`
	From             string         `json:"from"`
	Phase            string         `json:"phase"`
	InputStep        int            `json:"inputStep"`
	PatchTransitions int            `json:"patchTransitions"`
}
type Driver struct {
	Now       func() time.Time
	State     State
	Backend   Backend
	Log       string
	ResultDir string
	Waiting   string
}

// Load never invents a replacement nonce after a damaged or missing log.
func Load(path string, backend Backend) (*Driver, error) {
	events, err := journal.ReadMemoryLog(path)
	if err != nil {
		return nil, err
	}
	if len(events) == 0 {
		return nil, ErrJournalMissing
	}
	d := &Driver{Backend: backend, Log: path, ResultDir: filepath.Join(filepath.Dir(filepath.Dir(path)), "results")}
	if err = json.Unmarshal(events[len(events)-1].Data, &d.State); err != nil {
		return nil, err
	}
	if err = hydrate(path, &d.State); err != nil {
		return nil, err
	}
	return d, nil
}
func New(path string, backend Backend, identity Identity) (*Driver, error) {
	if err := identity.Validate(); err != nil {
		return nil, err
	}
	id, err := token()
	if err != nil {
		return nil, err
	}
	return &Driver{State: State{Schema: "lycheedev.channel.v2", ID: "CON-" + id, Owner: id, Identity: identity, ConnectBudget: NewDurableBudget(time.Now(), DefaultConnectBudget, false)}, Backend: backend, Log: path, ResultDir: filepath.Join(filepath.Dir(filepath.Dir(path)), "results")}, nil
}
func (d *Driver) Save(ctx context.Context, kind string) error {
	if d.State.Schema == "lycheedev.channel.v1" {
		d.State.Schema = "lycheedev.channel.v2"
	}
	if kind != "budget_observed" && kind != "budget_migrated" && kind != "blocker_observed" {
		d.State.ProgressVersion++
	}
	if err := d.State.validate(); err != nil {
		return err
	}
	value, err := snapshot(ctx, d.Log, d.State)
	if err != nil {
		return err
	}
	return journal.AppendMemoryEvent(ctx, d.Log, kind, value)
}

func (d *Driver) begin(ctx context.Context, action, ticket string, configure func(*bridge.SlotEnvelope)) error {
	if d.State.RuntimeEnd != nil {
		return runtimeRetirementPending()
	}
	if d.State.Transaction != nil {
		return errors.New("live.channel_transaction_pending")
	}
	nonce, err := token()
	if err != nil {
		return err
	}
	i := d.State.Identity
	e := bridge.SlotEnvelope{Schema: bridge.SlotSchema, Index: i.NextSlot, Runtime: i.Runtime, Owner: d.State.Owner, Fence: 1, Nonce: nonce, Ticket: ticket, Action: action, GUID: i.GUID, Build: i.Build}
	e.StartSlot = i.NextSlot
	if configure != nil {
		configure(&e)
	}
	if _, err = bridge.SlotPayload(e); err != nil {
		return err
	}
	d.State.Transaction = &Transaction{Envelope: e, Phase: "intent"}
	return d.Save(ctx, "slot_intent")
}

// advance records intent before every file/input side effect. Once input was
// attempted, replay is forbidden: retry only reads the original nonce.
func (d *Driver) advance(ctx context.Context) (*Receipt, error) {
	if d.State.RuntimeEnd != nil {
		return nil, runtimeRetirementPending()
	}
	tx := d.State.Transaction
	if tx == nil {
		return nil, errors.New("live.channel_transaction_missing")
	}
	if tx.Receipt != nil {
		if tx.Receipt.State == "rejected" {
			return tx.Receipt, &RejectedError{Reason: tx.Receipt.Reason}
		}
		return tx.Receipt, nil
	}
	recoveryBinding := d.State.Recovery != nil && d.State.Recovery.Phase != "complete" || d.State.Reload != nil && d.State.Reload.Phase == "binding"
	if d.State.Closing && (tx.Envelope.Action == "prepare" || tx.Envelope.Action == "commit" || tx.Envelope.Action == "bind" && !recoveryBinding) {
		// A durable intent may already have reached disk before the host
		// crashed. Closing observes it; it never publishes or wakes business.
		d.Waiting = "closing_exchange_unconfirmed"
		return d.receive(ctx, tx)
	}
	for tx.Phase == "intent" {
		if err := d.Backend.Publish(ctx, tx.Envelope); err != nil {
			var move *slotAvailable
			if errors.As(err, &move) && tx.Envelope.Schema == bridge.SlotSchema && move.Index > tx.Envelope.Index && move.Index <= bridge.SlotCount && (d.State.Input == nil || d.State.Input.Exchange != tx.Envelope.Nonce) {
				// Publish proved that this index belongs to another reservation,
				// before any write. Persist the new destination before trying it;
				// a crash or allocation race retains this nonce and route origin.
				tx.Envelope.Index = move.Index
				if err := d.Save(ctx, "slot_reallocated"); err != nil {
					return nil, err
				}
				continue
			}
			return nil, err
		}
		tx.Phase = "published"
		if err := d.Save(ctx, "slot_published"); err != nil {
			return nil, err
		}
	}
	if tx.Phase == "published" {
		if err := d.invoke(ctx, tx); err != nil {
			return nil, err
		}
	}
	return d.receive(ctx, tx)
}

func (d *Driver) receive(ctx context.Context, tx *Transaction) (*Receipt, error) {
	observation, err := d.Backend.Observe(ctx, ObservationQuery{Kind: "receipt", Envelope: tx.Envelope, Identity: d.State.Identity})
	r := observation.Receipt
	var rejection *RejectedError
	if err != nil && !errors.As(err, &rejection) {
		if errors.Is(err, ErrPending) {
			if diagnosticErr := d.recordInputDiagnostic(ctx, tx, observation.InputDiagnostic); diagnosticErr != nil {
				return nil, errors.Join(err, diagnosticErr)
			}
		}
		return nil, err
	}
	// Persist receipt before releasing the shared disk slot reservation.
	tx.Receipt = &r
	tx.Phase = "received"
	if err = d.Save(ctx, "receipt_verified"); err != nil {
		return nil, err
	}
	if rejection != nil {
		return &r, rejection
	}
	return &r, nil
}
func (d *Driver) finishTransaction(ctx context.Context) error {
	tx := d.State.Transaction
	if tx == nil || tx.Receipt == nil {
		return errors.New("live.channel_receipt_required")
	}
	if err := d.Backend.Consumed(ctx, tx.Envelope); err != nil {
		return err
	}
	d.State.Identity = tx.Receipt.Identity
	d.State.Identity.Schema = IdentitySchema
	d.State.Transaction = nil
	return d.Save(ctx, "slot_consumed")
}

func (d *Driver) Connect(ctx context.Context) error {
	if d.State.Bound {
		return nil
	}
	if d.State.Transaction == nil {
		if err := d.begin(ctx, "bind", d.State.Owner, nil); err != nil {
			return err
		}
	}
	r, err := d.advance(ctx)
	if err != nil {
		return err
	}
	if r.State != "bound" {
		return errors.New("live.channel_bind_not_confirmed")
	}
	d.State.Bound = true
	return d.finishTransaction(ctx)
}

// RecoverBinding consumes an explicit runtime-changed rejection for the exact
// bootstrap nonce. It only selects a new candidate; a NEW binding challenge
// must still succeed before this connection can execute business code.
func (d *Driver) RecoverBinding(ctx context.Context) error {
	if d.State.RuntimeEnd != nil {
		return runtimeRetirementPending()
	}
	tx := d.State.Transaction
	if d.State.Bound || d.State.Operation != nil || tx == nil || tx.Envelope.Action != "bind" {
		return errors.New("live.channel_bootstrap_recovery_unsafe")
	}
	e := tx.Envelope
	observation, err := d.Backend.Observe(ctx, ObservationQuery{Kind: "bootstrap_changed", Envelope: e, Identity: d.State.Identity})
	if err != nil {
		return err
	}
	receipt := observation.Receipt
	tx.Receipt = &receipt
	tx.Phase = "received"
	if err = d.Save(ctx, "bootstrap_runtime_rejected"); err != nil {
		return err
	}
	if err = d.finishTransaction(ctx); err != nil {
		return err
	}
	if d.State.Closing {
		return nil
	}
	return d.Connect(ctx)
}
func (d *Driver) PrepareOperation(ctx context.Context, code string, budget int, policy string) error {
	return d.PrepareRequest(ctx, "", code, budget, policy)
}

func (d *Driver) PrepareRequest(ctx context.Context, request, code string, budget int, policy string) error {
	if d.State.RuntimeEnd != nil {
		return runtimeRetirementPending()
	}
	if d.State.Closing || d.State.Closed {
		return errors.New("live.channel_stopping")
	}
	if !d.State.Bound || d.State.Transaction != nil {
		return errors.New("live.channel_not_idle")
	}
	if d.State.Operation != nil && d.State.Operation.Stage != "complete" {
		return ErrPending
	}
	if len(code) < 1 || len(code) > 262144 || code[0] == 27 || budget < 1 || budget > 120 || (policy != "observation" && policy != "opaque") {
		return errors.New("live.channel_request_invalid")
	}
	if err := d.checkpoint(ctx); err != nil {
		return err
	}
	id, err := token()
	if err != nil {
		return err
	}
	ticket, err := token()
	if err != nil {
		return err
	}
	origin := d.State.Identity
	d.State.Operation = &Operation{ID: "LMO-" + id, Ticket: ticket, Request: request, Origin: &origin, Attempt: 1, Code: code, Budget: budget, Policy: policy, Stage: "prepared", RecoveryBudget: NewDurableBudget(d.now(), DefaultRecoveryBudget, false)}
	return d.Save(ctx, "operation_created")
}

// Run is restartable at every durable boundary. It cannot repeat a commit.
// A verified result is written durably before the release request is admitted.
func (d *Driver) Run(ctx context.Context) error {
	op := d.State.Operation
	if op == nil {
		return errors.New("live.channel_operation_missing")
	}
	for {
		if err := ctx.Err(); err != nil {
			return err
		}
		if d.State.Closing {
			if op.Stage == "prepared" && d.State.Transaction == nil {
				op.Stage = "cancelled"
				return d.Save(ctx, "operation_cancelled_before_publication")
			}
			if op.Stage == "commit_ready" && d.State.Transaction == nil {
				d.Waiting = "prepared_operation_requires_reload"
				return ErrPending
			}
		}
		switch op.Stage {
		case "execution_unknown":
			return ErrExecutionUnknown
		case "complete", "cancelled":
			return nil
		case "prepared":
			if d.State.Transaction == nil && d.State.Identity.NextSlot > d.State.Identity.Slots-15 {
				return ErrReloadRequired
			}
			if d.State.Transaction == nil {
				if err := d.begin(ctx, "prepare", op.Ticket, func(e *bridge.SlotEnvelope) {
					e.Code = op.Code
					e.CodeBytes = len(op.Code)
					e.CodeChecksum = adler32.Checksum([]byte(op.Code))
					e.Budget = op.Budget
				}); err != nil {
					return err
				}
			}
			r, err := d.advance(ctx)
			if err != nil {
				return err
			}
			if (r.State != "prepared" && r.State != "reported") || len(r.Challenge) != 32 {
				return errors.New("live.channel_prepare_mismatch")
			}
			op.PreparedNonce = d.State.Transaction.Envelope.Nonce
			op.Challenge = r.Challenge
			op.Stage = "commit_ready"
			if r.State == "reported" {
				op.Stage = "running"
			}
			if err = d.finishTransaction(ctx); err != nil {
				return err
			}
		case "commit_ready":
			if d.State.Transaction == nil {
				if err := d.begin(ctx, "commit", op.Ticket, func(e *bridge.SlotEnvelope) { e.Challenge = op.Challenge; e.PreparedNonce = op.PreparedNonce }); err != nil {
					return err
				}
			}
			r, err := d.advance(ctx)
			if err != nil {
				return err
			}
			if r.State != "accepted" {
				return errors.New("live.channel_commit_not_accepted")
			}
			op.Stage = "running"
			if err = d.finishTransaction(ctx); err != nil {
				return err
			}
		case "running":
			i := d.State.Identity
			e := bridge.SlotEnvelope{Nonce: op.PreparedNonce, Runtime: i.Runtime, Ticket: op.Ticket, Owner: d.State.Owner, Fence: i.Fence, GUID: i.GUID, Build: i.Build}
			observation, err := d.Backend.Observe(ctx, ObservationQuery{Kind: "result", Envelope: e, Identity: i})
			if err != nil {
				return err
			}
			head := observation.Receipt
			op.Result = append([]byte(nil), observation.Payload...)
			op.ReportBytes = head.ReportBytes
			op.ReportChecksum = head.ReportChecksum
			op.Stage = "confirm_ready"
			if err = d.Save(ctx, "report_candidate"); err != nil {
				return err
			}
		case "confirm_ready":
			if d.State.Transaction == nil {
				if err := d.begin(ctx, "confirm", op.Ticket, nil); err != nil {
					return err
				}
			}
			r, err := d.advance(ctx)
			if err != nil {
				return err
			}
			if r.State != "reported" || r.PreparedNonce != op.PreparedNonce || r.ReportBytes != op.ReportBytes || r.ReportChecksum != op.ReportChecksum {
				return errors.New("live.channel_report_not_current")
			}
			// The result journal and standalone artifact both precede game release.
			op.Stage = "result_verified"
			if err = d.finishTransaction(ctx); err != nil {
				return err
			}
		case "result_verified":
			b, err := json.Marshal(op)
			if err != nil {
				return err
			}
			if err = vault.ReplaceFile(ctx, filepath.Join(d.ResultDir, op.ID+".json"), b); err != nil {
				return err
			}
			op.Stage = "release_ready"
			if err = d.Save(ctx, "result_durable"); err != nil {
				return err
			}
		case "release_ready":
			if op.CleanupMethod == "reload_required" {
				return ErrCleanupReloadRequired
			}
			if d.State.Transaction == nil {
				if err := d.begin(ctx, "release", op.Ticket, func(e *bridge.SlotEnvelope) { e.ReportBytes = op.ReportBytes; e.ReportChecksum = op.ReportChecksum }); err != nil {
					return err
				}
			}
			r, err := d.advance(ctx)
			if err != nil {
				var rejection *RejectedError
				if errors.As(err, &rejection) && rejection.Reason == "slot_resources_pending" {
					op.CleanupMethod = "reload_required"
					if err = d.finishTransaction(ctx); err != nil {
						return err
					}
					return ErrCleanupReloadRequired
				}
				return err
			}
			if r.State != "released" {
				return errors.New("live.channel_release_not_confirmed")
			}
			op.Stage = "complete"
			op.CleanupMethod = "released"
			if err = d.finishTransaction(ctx); err != nil {
				return err
			}
		default:
			return fmt.Errorf("live.channel_stage_invalid: %s", op.Stage)
		}
	}
}

func (d *Driver) Disconnect(ctx context.Context) error {
	if d.State.Transaction != nil && d.State.Transaction.Envelope.Action != "unbind" {
		return ErrPending
	}
	if !d.State.Bound {
		return nil
	}
	if d.State.Operation != nil && d.State.Operation.Stage != "complete" && d.State.Operation.Stage != "execution_unknown" && d.State.Operation.Stage != "cancelled" {
		return ErrPending
	}
	if d.State.Transaction == nil {
		if err := d.checkpoint(ctx); err != nil {
			return err
		}
		if err := d.begin(ctx, "unbind", d.State.Owner, nil); err != nil {
			return err
		}
	}
	r, err := d.advance(ctx)
	if err != nil {
		return err
	}
	if r.State != "unbound" {
		return errors.New("live.channel_unbind_not_confirmed")
	}
	d.State.Bound = false
	return d.finishTransaction(ctx)
}
