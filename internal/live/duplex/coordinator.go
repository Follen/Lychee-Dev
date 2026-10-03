package duplex

import (
	"context"
	"encoding/hex"
	"errors"
	"fmt"
	"math"
	"time"
)

var (
	ErrPersistence = errors.New("duplex durable persistence failed")
	ErrRejected    = errors.New("duplex exact control was rejected")
	ErrPending     = errors.New("duplex operation pending; resume original identity")
	ErrBusy        = errors.New("duplex business request is outstanding")
	ErrIdentity    = errors.New("duplex identity changed")
	ErrUnknown     = errors.New("duplex execution or publication is unknown; no automatic replay")
	ErrBudget      = errors.New("duplex original transfer deadline exhausted; execution outcome retained")
)

// Backend must re-observe exact OS/runtime/arena identity and guard the layout
// before every write. Observe and ReadResult are read-only. Publish performs at
// most one publication and reports actual write facts, including on errors.
// Controls must use independent lane locks; Frame cannot monopolize controls.
type Backend interface {
	Observe(context.Context) (Sendbox, error)
	Publish(context.Context, Message) (WriteOutcome, error)
	ReadResult(context.Context, ResultManifest) ([]byte, error)
	Close(context.Context) error
}

type Coordinator struct {
	Backend      Backend
	Store        Store
	PollInterval time.Duration
	Now          func() time.Time
	// Host transfer deadline is independent of the immutable addon execution
	// budget. 600 seconds is a bounded candidate, not a measured throughput.
	TransferBudgetMillis uint32
}

func NewCoordinator(b Backend, s Store) *Coordinator {
	return &Coordinator{Backend: b, Store: s, PollInterval: 25 * time.Millisecond, Now: time.Now, TransferBudgetMillis: 600000}
}
func (c *Coordinator) now() time.Time {
	if c.Now != nil {
		return c.Now()
	}
	return time.Now()
}
func (c *Coordinator) wait(ctx context.Context) error {
	d := c.PollInterval
	if d <= 0 {
		d = 25 * time.Millisecond
	}
	if d > 250*time.Millisecond {
		d = 250 * time.Millisecond
	}
	t := time.NewTimer(d)
	defer t.Stop()
	select {
	case <-ctx.Done():
		return ctx.Err()
	case <-t.C:
		return nil
	}
}
func exact(id Identity, s Sendbox) bool { return id == s.Identity }
func requestMatches(a *ActiveRequest, r *RequestState) bool {
	return a != nil && r != nil && a.RequestID == r.RequestID && a.Digest == r.RequestSHA256 && a.Sequence == r.RequestSeq && a.Attempt == r.TransportAttempt
}
func releasedMatches(a *ActiveRequest, r *Released) bool {
	return a != nil && r != nil && a.RequestID == r.RequestID && a.Digest == r.RequestSHA256
}

func rejectedReceipt(state string) bool {
	switch state {
	case "rejected", "expired", "owner_conflict", "actor_binding_mismatch", "bind_identity_invalid", "commit_rejected", "actor_changed", "clock_unavailable", "challenge_expired", "reload_blocked", "reload_unavailable", "reload_expired", "reload_lease_rejected", "ack_rejected", "ack_mismatch", "repair_rejected", "unsupported", "cancel_rejected":
		return true
	}
	return false
}

func (c *Coordinator) Status(ctx context.Context) (State, Sendbox, error) {
	st, e := c.Store.Load(ctx)
	if e != nil {
		return st, Sendbox{}, e
	}
	s, e := c.Backend.Observe(ctx)
	return st, s, e
}

func (c *Coordinator) Connect(ctx context.Context, id Identity) (State, error) {
	for _, v := range []string{id.Runtime, id.Arena, id.Session, id.Owner, id.ActorBinding} {
		if _, e := token(v); e != nil {
			return State{}, e
		}
	}
	if id.Fence == 0 {
		return State{}, errors.New("zero owner fence")
	}
	first, e := c.Backend.Observe(ctx)
	if e != nil {
		return State{}, e
	}
	if first.Runtime != id.Runtime || first.Arena != id.Arena || first.ActorBinding != id.ActorBinding {
		return State{}, ErrIdentity
	}
	if !first.ControlReady {
		return State{}, ErrPending
	}
	var bindPayload []byte
	if !exact(id, first) && first.Owner != "00000000000000000000000000000000" {
		bindPayload, e = EncodeClosedBindProof(first)
		if e != nil {
			return State{}, e
		}
	}
	h := Header{Kind: Bind, Runtime: id.Runtime, Arena: id.Arena, Session: id.Session, Owner: id.Owner, ActorBinding: id.ActorBinding, Fence: id.Fence, RequestID: "00000000000000000000000000000000", RequestSHA256: "0000000000000000000000000000000000000000000000000000000000000000"}
	if e = ValidateBindTransition(first, Message{Header: h, Payload: bindPayload}); e != nil {
		return State{}, e
	}
	// A static ready projection cannot authorize even the initial bind.
	for {
		if e = c.wait(ctx); e != nil {
			return State{}, e
		}
		second, err := c.Backend.Observe(ctx)
		if err != nil {
			return State{}, err
		}
		if second.Identity != first.Identity || second.Phase != first.Phase || !releasedEqual(second.Released, first.Released) || second.ResourcesReleased != first.ResourcesReleased {
			return State{}, ErrIdentity
		}
		if second.StatusSequence < first.StatusSequence || second.Heartbeat < first.Heartbeat {
			return State{}, ErrIdentity
		}
		if second.StatusSequence > first.StatusSequence && second.Heartbeat > first.Heartbeat {
			break
		}
	}
	e = c.Store.Update(ctx, func(st *State) error {
		if in, ok := st.Intents["bindResume"]; ok && in.Message.Header.Kind == Repair && !in.Accepted {
			return ErrPending
		}
		if st.Bound && st.Identity != id {
			return ErrBusy
		}
		if st.Active != nil && !st.Active.Released {
			return ErrBusy
		}
		st.Identity = id
		st.Closed = false
		return nil
	})
	if e != nil {
		return State{}, e
	}
	if e = c.publishControl(ctx, Bind, "00000000000000000000000000000000", bindPayload); e != nil && !errors.Is(e, ErrPending) {
		return State{}, e
	}
	return c.Resume(ctx)
}

func (c *Coordinator) Execute(ctx context.Context, source []byte, budgetMillis uint32) (State, error) {
	if len(source) > MaxSourceBytes || budgetMillis == 0 || budgetMillis > 120000 {
		return State{}, errors.New("business budget must be 1..120000 ms and source <=1 MiB")
	}
	s, e := c.Backend.Observe(ctx)
	if e != nil {
		return State{}, e
	}
	if e = validateSendbox(s); e != nil {
		return State{}, e
	}
	if !s.BusinessReady || !s.TransportReady {
		return State{}, ErrBusy
	}
	e = c.Store.Update(ctx, func(st *State) error {
		if !st.Bound || st.Closed || !exact(st.Identity, s) {
			return ErrIdentity
		}
		if in, ok := st.Intents["bindResume"]; ok && in.Message.Header.Kind == Repair && !in.Accepted {
			return ErrPending
		}
		if st.Active != nil && !st.Active.Released {
			return ErrBusy
		}
		if st.RequestSequence == math.MaxUint64 {
			return errors.New("request sequence exhausted")
		}
		rid, err := NewToken()
		if err != nil {
			return err
		}
		seq := st.RequestSequence + 1
		created := uint64(c.now().UnixMilli())
		transferBudget := c.TransferBudgetMillis
		if transferBudget == 0 {
			transferBudget = 600000
		}
		if transferBudget > 600000 {
			return errors.New("transfer budget exceeds 600000 ms")
		}
		frames, err := NewFrames(st.Identity, rid, seq, 1, created, budgetMillis, source)
		if err != nil {
			return err
		}
		st.RequestSequence = seq
		st.Active = &ActiveRequest{RequestID: rid, Digest: frames[0].Header.RequestSHA256, Sequence: seq, Attempt: 1, Created: created, Budget: budgetMillis, TransferDeadline: created + uint64(transferBudget), Source: append([]byte(nil), source...), Frames: frames, Phase: "receiving"}
		delete(st.Intents, "request")
		delete(st.Intents, "commit")
		delete(st.Intents, "resultAck")
		return nil
	})
	if e != nil {
		return State{}, e
	}
	return c.Resume(ctx)
}

func (c *Coordinator) publish(ctx context.Context, m Message, prepare ...func(*State) error) error {
	lane := m.Header.Kind.Lane()
	var intent Intent
	e := c.Store.Update(ctx, func(st *State) error {
		if old, ok := st.Intents[lane]; ok && !old.Accepted && old.Outcome.State != NoWrite {
			return ErrPending
		}
		if st.PublicationSequence >= math.MaxUint64/2 {
			return errors.New("publication sequence exhausted")
		}
		for _, f := range prepare {
			if e := f(st); e != nil {
				return e
			}
		}
		st.PublicationSequence++
		m.Header.PublicationSeq = st.PublicationSequence
		m.Header.PublicationBegin = st.PublicationSequence * 2
		m.Header.PublicationEnd = m.Header.PublicationBegin
		wire, err := EncodeMessage(m)
		if err != nil {
			return err
		}
		m, err = DecodeMessage(wire)
		if err != nil {
			return err
		}
		intent = Intent{Message: m, Outcome: WriteOutcome{State: UnknownWrite}}
		st.Intents[lane] = intent
		return nil
	})
	if e != nil {
		return e
	}
	out, writeErr := c.Backend.Publish(ctx, m)
	switch out.State {
	case NoWrite, CompleteWrite, PartialWrite, UnknownWrite:
	default:
		out.State = UnknownWrite
	}
	// Even when the caller cancelled, attempt to record the write facts within
	// an independent bounded local durability window.
	durableCtx, cancel := context.WithTimeout(context.WithoutCancel(ctx), 2*time.Second)
	defer cancel()
	e = c.Store.Update(durableCtx, func(st *State) error {
		old, ok := st.Intents[lane]
		if !ok || old.Message.Header.MessageID != intent.Message.Header.MessageID {
			return ErrIdentity
		}
		old.Outcome = out
		st.Intents[lane] = old
		return nil
	})
	if e != nil {
		return errors.Join(ErrPersistence, fmt.Errorf("publication outcome not durable: %w", e))
	}
	if writeErr != nil {
		return writeErr
	}
	if out.State != CompleteWrite {
		return ErrUnknown
	}
	return nil
}

func (c *Coordinator) publishControl(ctx context.Context, kind Kind, challenge string, payload []byte) error {
	st, e := c.Store.Load(ctx)
	if e != nil {
		return e
	}
	mid, e := NewToken()
	if e != nil {
		return e
	}
	id := st.Identity
	h := Header{Kind: kind, Runtime: id.Runtime, Arena: id.Arena, Session: id.Session, Owner: id.Owner, ActorBinding: id.ActorBinding, Fence: id.Fence, RequestID: "00000000000000000000000000000000", MessageID: mid, Challenge: challenge, RequestSHA256: "0000000000000000000000000000000000000000000000000000000000000000", TransportAttempt: 1, CreatedUTCMillis: uint64(c.now().UnixMilli()), PayloadBytes: uint32(len(payload))}
	if a := st.Active; a != nil && !a.Released {
		h.RequestID = a.RequestID
		h.RequestSeq = a.Sequence
		h.RequestSHA256 = a.Digest
		h.TransportAttempt = a.Attempt
		h.CreatedUTCMillis = a.Created
		h.BudgetMillis = a.Budget
		h.TotalBytes = uint32(len(a.Source))
	}
	return c.publish(ctx, Message{h, payload})
}

// reconcile never infers execution from bytes-written. Only exact current
// private-protocol receipts, request projections and tombstones advance facts.
func (c *Coordinator) reconcile(ctx context.Context, s Sendbox) error {
	return c.Store.Update(ctx, func(st *State) error {
		if !exact(st.Identity, s) {
			return ErrIdentity
		}
		for lane, in := range st.Intents {
			r, ok := s.Receipts[lane]
			h := in.Message.Header
			if ok && r.MessageID == h.MessageID && r.RequestID == h.RequestID && r.RequestSHA256 == h.RequestSHA256 {
				switch r.State {
				case "accepted", "released", "closed", "closing", "cancel_requested", "not_started", "lease_observed", "commit_accepted", "cancelled", "cancel_too_late", "bound", "repaired", "renewed":
					in.Accepted = true
					in.Challenge = r.Challenge
					st.Intents[lane] = in
				default:
					if rejectedReceipt(r.State) {
						in.Rejected = r.State
						st.Intents[lane] = in
					}
				}
			}
		}
		if in, ok := st.Intents["bindResume"]; ok && in.Accepted && in.Message.Header.Kind == Bind {
			st.Bound = true
		}
		if in, ok := st.Intents["close"]; ok && in.Accepted {
			st.Closed = s.Phase == "closed"
		}
		if a := st.Active; a != nil && !a.Released {
			if requestMatches(a, s.Request) {
				a.Phase = s.Phase
				if !s.Request.NotStarted && a.ExecutionObservedHostAt == 0 {
					a.ExecutionObservedHostAt = uint64(c.now().UnixMilli())
				}
				if in, ok := st.Intents["request"]; ok {
					for _, n := range s.Request.AcceptedFrames {
						if n == in.Message.Header.FrameIndex {
							in.Accepted = true
							st.Intents["request"] = in
							if n == a.NextFrame+1 {
								a.NextFrame = n
							}
							break
						}
					}
				}
			}
			if releasedMatches(a, s.Released) {
				if !s.ResourcesReleased {
					return nil
				}
				if !a.ResultSaved {
					return errors.New("release observed before host result durability")
				}
				ack, ok := st.Intents["resultAck"]
				if !ok {
					return errors.New("release without result ACK intent")
				}
				ack.Accepted = true
				st.Intents["resultAck"] = ack
				a.Released = true
				a.Phase = "released"
				a.Source = nil
				a.Frames = nil
			}
		}
		return nil
	})
}

// Step performs at most one side effect. A caller can integrate it with its own
// bounded observer loop; Resume is the convenience loop for the same journal.
func (c *Coordinator) Step(ctx context.Context) (State, error) {
	s, e := c.Backend.Observe(ctx)
	if e != nil {
		return State{}, e
	}
	if e = validateSendbox(s); e != nil {
		return State{}, e
	}
	prior, loadErr := c.Store.Load(ctx)
	if loadErr != nil {
		return prior, loadErr
	}
	if !prior.Bound && !exact(prior.Identity, s) {
		if in, ok := prior.Intents["bindResume"]; ok && in.Message.Header.Kind == Bind && ValidateBindTransition(s, in.Message) == nil {
			if r, ok := s.Receipts["bindResume"]; ok && r.MessageID == in.Message.Header.MessageID && r.RequestID == in.Message.Header.RequestID && r.RequestSHA256 == in.Message.Header.RequestSHA256 && rejectedReceipt(r.State) {
				e = c.Store.Update(ctx, func(st *State) error {
					current := st.Intents["bindResume"]
					if current.Message.Header.MessageID != r.MessageID {
						return ErrIdentity
					}
					current.Rejected = r.State
					st.Intents["bindResume"] = current
					return nil
				})
				return prior, errors.Join(e, fmt.Errorf("%w: bindResume %s", ErrRejected, r.State))
			}
			if in.Outcome.State == NoWrite {
				return prior, c.publish(ctx, in.Message)
			}
			return prior, ErrPending
		}
	}
	if e = c.reconcile(ctx, s); e != nil {
		return State{}, e
	}
	st, e := c.Store.Load(ctx)
	if e != nil {
		return st, e
	}
	a := st.Active
	if in, ok := st.Intents["bindResume"]; ok && in.Rejected != "" {
		return st, fmt.Errorf("%w: bindResume %s", ErrRejected, in.Rejected)
	}
	if a != nil && !a.Released {
		for _, lane := range []string{"request", "commit", "resultAck"} {
			if lane != "resultAck" && s.Terminal != nil && s.Terminal.RequestID == a.RequestID && s.Terminal.RequestSHA256 == a.Digest {
				continue
			}
			if in, ok := st.Intents[lane]; ok && in.Message.Header.RequestID == a.RequestID && in.Message.Header.RequestSHA256 == a.Digest && in.Rejected != "" {
				return st, fmt.Errorf("%w: %s %s", ErrRejected, lane, in.Rejected)
			}
		}
	}
	if !st.Bound {
		return st, ErrPending
	}
	if in, ok := st.Intents["bindResume"]; ok && in.Message.Header.Kind == Repair && !in.Accepted {
		if in.Outcome.State == NoWrite {
			e = c.publish(ctx, in.Message)
			return st, e
		}
		return st, ErrPending
	}
	if a == nil || a.Released {
		return st, nil
	}
	if s.Terminal != nil && s.Terminal.RequestID == a.RequestID && s.Terminal.RequestSHA256 == a.Digest {
		m := *s.Terminal
		if !a.ResultSaved {
			p, err := c.Backend.ReadResult(ctx, m)
			if err != nil {
				return st, err
			}
			if err = ValidateResult(m, p); err != nil {
				return st, err
			}
			if err = c.Store.SaveResult(ctx, m, p); err != nil {
				return st, errors.Join(ErrPersistence, err)
			}
			e = c.Store.Update(ctx, func(current *State) error {
				if current.Active == nil || current.Active.RequestID != a.RequestID {
					return ErrIdentity
				}
				current.Active.Result = &m
				current.Active.ResultSaved = true
				current.Active.Phase = "result_pending"
				return nil
			})
			return st, e
		}
		if in, ok := st.Intents["resultAck"]; !ok || in.Outcome.State == NoWrite {
			p, err := EncodeResultAck(m)
			if err != nil {
				return st, err
			}
			e = c.publishControl(ctx, ResultAck, "00000000000000000000000000000000", p)
			return st, e
		}
		return st, ErrPending
	}
	// Budget does not prevent result collection or independent cleanup controls.
	if a.ExecutionObservedHostAt == 0 && c.now().UnixMilli() >= int64(a.TransferDeadline) {
		return st, ErrBudget
	}
	if st.Closed || s.Phase == "closed" {
		return st, ErrPending
	}
	if !s.TransportReady {
		return st, ErrPending
	}
	if a.NextFrame < uint32(len(a.Frames)) {
		if in, ok := st.Intents["request"]; ok && !in.Accepted && in.Outcome.State != NoWrite {
			return st, ErrPending
		}
		if s.Request != nil && !requestMatches(a, s.Request) {
			return st, ErrBusy
		}
		e = c.publish(ctx, a.Frames[a.NextFrame])
		return st, e
	}
	if requestMatches(a, s.Request) && s.Phase == "prepared" && s.Request.NotStarted {
		if s.Request.Challenge == "" || s.Request.Challenge == "00000000000000000000000000000000" {
			return st, errors.New("missing prepare challenge")
		}
		if in, ok := st.Intents["commit"]; ok && !in.Accepted && in.Outcome.State != NoWrite {
			return st, ErrPending
		}
		if in, ok := st.Intents["commit"]; !ok || in.Outcome.State == NoWrite {
			e = c.publishControl(ctx, Commit, s.Request.Challenge, nil)
			return st, e
		}
	}
	return st, ErrPending
}

func (c *Coordinator) Resume(ctx context.Context) (State, error) {
	for {
		st, e := c.Step(ctx)
		if e == nil && (st.Active == nil || st.Active.Released) && st.Bound {
			return st, nil
		}
		if e != nil && !errors.Is(e, ErrPending) {
			return st, e
		}
		if e = c.wait(ctx); e != nil {
			durableCtx, cancel := context.WithTimeout(context.WithoutCancel(ctx), 2*time.Second)
			latest, _ := c.Store.Load(durableCtx)
			cancel()
			return latest, ErrPending
		}
	}
}

func (c *Coordinator) control(ctx context.Context, kind Kind, cleanupBudget time.Duration) (State, error) {
	if cleanupBudget <= 0 {
		return State{}, errors.New("cleanup budget must be positive")
	}
	ctx, cancel := context.WithTimeout(ctx, cleanupBudget)
	defer cancel()
	st, e := c.Store.Load(ctx)
	if e != nil {
		return st, e
	}
	s, e := c.Backend.Observe(ctx)
	if e != nil {
		return st, e
	}
	if e = validateSendbox(s); e != nil {
		return st, e
	}
	if !exact(st.Identity, s) || !s.ControlReady {
		return st, ErrIdentity
	}
	if e = c.reconcile(ctx, s); e != nil {
		return st, e
	}
	lane := kind.Lane()
	st, e = c.Store.Load(ctx)
	if e != nil {
		return st, e
	}
	if in, ok := st.Intents[lane]; ok && in.Rejected != "" {
		return st, fmt.Errorf("%w: %s %s", ErrRejected, lane, in.Rejected)
	}
	if in, ok := st.Intents[lane]; !ok || in.Accepted && kind != Reload || in.Outcome.State == NoWrite {
		if e = c.publishControl(ctx, kind, "00000000000000000000000000000000", nil); e != nil && !errors.Is(e, ErrPending) {
			return st, e
		}
	}
	for {
		s, e = c.Backend.Observe(ctx)
		if e != nil {
			return st, e
		}
		if e = c.reconcile(ctx, s); e != nil {
			return st, e
		}
		st, e = c.Store.Load(ctx)
		if e != nil {
			return st, e
		}
		if in, ok := st.Intents[lane]; ok && in.Accepted {
			return st, nil
		}
		if in, ok := st.Intents[lane]; ok && in.Rejected != "" {
			return st, fmt.Errorf("%w: %s %s", ErrRejected, lane, in.Rejected)
		}
		if e = c.wait(ctx); e != nil {
			return st, ErrPending
		}
	}
}
func (c *Coordinator) Cancel(ctx context.Context, budget time.Duration) (State, error) {
	return c.control(ctx, Cancel, budget)
}

// Reload records an exact control and observes its original-runtime receipt.
// A runtime transition before that receipt remains unknown; no request is
// rebound or replayed into the replacement runtime.
func (c *Coordinator) Reload(ctx context.Context, budget time.Duration) (State, error) {
	return c.control(ctx, Reload, budget)
}

// CommitReload is a distinct lease publication after the prepared reload
// receipt (including its challenge) has become durable. Unknown publication is
// never replayed; a replacement runtime can only retire the original session.
func (c *Coordinator) CommitReload(ctx context.Context, budget time.Duration) (State, error) {
	if budget <= 0 {
		return State{}, errors.New("cleanup budget must be positive")
	}
	ctx, cancel := context.WithTimeout(ctx, budget)
	defer cancel()
	st, e := c.Store.Load(ctx)
	if e != nil {
		return st, e
	}
	prepared, ok := st.Intents["reload"]
	if !ok || !prepared.Accepted || prepared.Challenge == "" || prepared.Challenge == "00000000000000000000000000000000" || st.Active != nil && !st.Active.Released {
		return st, ErrPending
	}
	payload, e := token(prepared.Message.Header.MessageID)
	if e != nil {
		return st, e
	}
	if _, e = token(prepared.Challenge); e != nil {
		return st, e
	}
	old, exists := st.Intents["lease"]
	same := exists && hex.EncodeToString(old.Message.Payload) == prepared.Message.Header.MessageID && old.Message.Header.Challenge == prepared.Challenge
	if !exists || old.Outcome.State == NoWrite || old.Accepted && !same {
		if e = c.publishControl(ctx, Lease, prepared.Challenge, payload); e != nil {
			return st, e
		}
	} else if !same {
		return st, ErrPending
	}
	for {
		s, err := c.Backend.Observe(ctx)
		if err != nil {
			return st, err
		}
		if err = c.reconcile(ctx, s); err != nil {
			return st, err
		}
		st, err = c.Store.Load(ctx)
		if err != nil {
			return st, err
		}
		if in, ok := st.Intents["lease"]; ok && in.Accepted && hex.EncodeToString(in.Message.Payload) == prepared.Message.Header.MessageID && in.Message.Header.Challenge == prepared.Challenge {
			return st, nil
		}
		if in, ok := st.Intents["lease"]; ok && in.Rejected != "" {
			return st, fmt.Errorf("%w: lease %s", ErrRejected, in.Rejected)
		}
		if err = c.wait(ctx); err != nil {
			return st, ErrPending
		}
	}
}
func (c *Coordinator) Disconnect(ctx context.Context, budget time.Duration) (State, error) {
	st, e := c.control(ctx, Close, budget)
	closeCtx, cancel := context.WithTimeout(context.WithoutCancel(ctx), 2*time.Second)
	defer cancel()
	closeErr := c.Backend.Close(closeCtx)
	if e != nil {
		return st, e
	}
	return st, closeErr
}

// Repair cannot adopt a new runtime or guess an old writer is dead. Both the
// transport drain and addon private ledger must prove the original not_started.
func (c *Coordinator) Repair(ctx context.Context, previousWriterDrained bool) (State, error) {
	st, e := c.Store.Load(ctx)
	if e != nil {
		return st, e
	}
	s, e := c.Backend.Observe(ctx)
	if e != nil {
		return st, e
	}
	p := s.Repair
	a := st.Active
	if !previousWriterDrained || p == nil || !p.LedgerRetained || p.PreviousArena == p.NewArena || p.PreviousArena != st.Identity.Arena || p.NewArena != s.Arena || s.Runtime != st.Identity.Runtime || s.Session != st.Identity.Session || s.Owner != st.Identity.Owner || s.ActorBinding != st.Identity.ActorBinding || s.Fence != st.Identity.Fence {
		return st, ErrUnknown
	}
	if p.Idle {
		return c.repairIdle(ctx, st, s, *p)
	}
	if !p.NotStarted || a == nil || a.Released || p.RequestID != a.RequestID || p.RequestSHA256 != a.Digest {
		return st, ErrUnknown
	}
	if in, ok := st.Intents["commit"]; ok && in.Accepted {
		return st, ErrUnknown
	}
	if a.PreviousCommit != nil {
		return st, ErrUnknown
	}
	if a.Attempt == math.MaxUint64 {
		return st, errors.New("transport attempt exhausted")
	}
	if c.now().UnixMilli() >= int64(a.TransferDeadline) {
		return st, ErrBudget
	}
	mid, e := NewToken()
	if e != nil {
		return st, e
	}
	id := st.Identity
	id.Arena = s.Arena
	h := Header{Kind: Repair, Runtime: id.Runtime, Arena: id.Arena, Session: id.Session, Owner: id.Owner, ActorBinding: id.ActorBinding, Fence: id.Fence, MessageID: mid, Challenge: p.Challenge, RequestID: a.RequestID, RequestSeq: a.Sequence, RequestSHA256: a.Digest, TransportAttempt: a.Attempt + 1, CreatedUTCMillis: a.Created, BudgetMillis: a.Budget, TotalBytes: uint32(len(a.Source))}
	e = c.publish(ctx, Message{Header: h}, func(current *State) error {
		if current.Identity != st.Identity || current.Active == nil || current.Active.RequestID != a.RequestID || current.Active.Attempt != a.Attempt {
			return ErrIdentity
		}
		if in, ok := current.Intents["commit"]; ok {
			current.Active.PreviousCommit = &in
			delete(current.Intents, "commit")
		}
		proof := *p
		current.Active.RepairProof = &proof
		current.Identity.Arena = s.Arena
		current.Active.Attempt++
		frames, err := NewFrames(current.Identity, a.RequestID, a.Sequence, current.Active.Attempt, a.Created, a.Budget, a.Source)
		if err != nil {
			return err
		}
		current.Active.Frames = frames
		current.Active.NextFrame = 0
		current.Active.Phase = "repairing"
		delete(current.Intents, "request")
		return nil
	})
	if e != nil {
		return st, e
	}
	return c.Store.Load(ctx)
}

// repairIdle changes only the physical arena. Both the host journal and the
// retained private ledger must prove there is no unacknowledged business. A
// released request remains in host history and the addon tombstone remains the
// sequence fence; neither is converted into a new request or owner.
func (c *Coordinator) repairIdle(ctx context.Context, st State, s Sendbox, p RepairProof) (State, error) {
	const zeroToken = "00000000000000000000000000000000"
	const zeroDigest = "0000000000000000000000000000000000000000000000000000000000000000"
	if !st.Bound || !p.NoPendingRequest || !p.ResourcesReleased || p.NotStarted || p.RequestID != zeroToken || p.RequestSHA256 != zeroDigest || !s.ResourcesReleased || s.Terminal != nil || s.Request != nil {
		return st, ErrUnknown
	}
	if a := st.Active; a != nil {
		if !a.Released || !a.ResultSaved || a.Result == nil || !releasedMatches(a, s.Released) {
			return st, ErrUnknown
		}
	}
	mid, e := NewToken()
	if e != nil {
		return st, e
	}
	id := st.Identity
	id.Arena = s.Arena
	h := Header{Kind: Repair, Runtime: id.Runtime, Arena: id.Arena, Session: id.Session, Owner: id.Owner, ActorBinding: id.ActorBinding, Fence: id.Fence, MessageID: mid, Challenge: p.Challenge, RequestID: zeroToken, RequestSHA256: zeroDigest, TransportAttempt: 1, CreatedUTCMillis: uint64(c.now().UnixMilli())}
	e = c.publish(ctx, Message{Header: h}, func(current *State) error {
		if current.Identity != st.Identity || !current.Bound || current.Active != nil && !current.Active.Released {
			return ErrIdentity
		}
		if st.Active != nil && (current.Active == nil || current.Active.RequestID != st.Active.RequestID || !current.Active.ResultSaved) {
			return ErrIdentity
		}
		proof := p
		current.RepairProof = &proof
		current.Identity.Arena = s.Arena
		return nil
	})
	if e != nil {
		return st, e
	}
	return c.Store.Load(ctx)
}
