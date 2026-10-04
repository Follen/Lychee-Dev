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
	ErrRejected    = errors.New("duplex exact publication was rejected")
	ErrPending     = errors.New("duplex operation pending; resume original identity")
	ErrBusy        = errors.New("duplex business request is outstanding")
	ErrIdentity    = errors.New("duplex identity changed")
	ErrUnknown     = errors.New("duplex execution or publication is unknown; no automatic replay")
	ErrBudget      = errors.New("duplex original transfer deadline exhausted; execution outcome retained")
)

// Backend re-observes the exact process/runtime/arena before every publication.
// Command and stop have independent locks; an uncertain write is never replayed.
type Backend interface {
	Observe(context.Context) (Sendbox, error)
	Publish(context.Context, Message) (WriteOutcome, error)
	ReadResult(context.Context, ResultManifest) ([]byte, error)
	Close(context.Context) error
}
type Coordinator struct {
	Backend              Backend
	Store                Store
	PollInterval         time.Duration
	Now                  func() time.Time
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

const zeroToken = "00000000000000000000000000000000"
const zeroDigest = "0000000000000000000000000000000000000000000000000000000000000000"

func exact(id Identity, s Sendbox) bool { return id == s.Identity }
func requestMatches(a *ActiveRequest, r *RequestState) bool {
	return a != nil && r != nil && a.RequestID == r.RequestID && a.Digest == r.RequestSHA256 && a.Sequence == r.RequestSeq && a.Attempt == r.TransportAttempt
}
func releasedMatches(a *ActiveRequest, r *Released) bool {
	return a != nil && r != nil && a.RequestID == r.RequestID && a.Digest == r.RequestSHA256
}
func rejectedReceipt(state string) bool {
	switch state {
	case "rejected", "expired", "owner_conflict", "actor_binding_mismatch", "actor_changed", "clock_unavailable", "challenge_expired", "ack_rejected", "ack_mismatch", "repair_rejected", "unsupported", "cancel_rejected", "challenge_mismatch", "identity_mismatch":
		return true
	}
	return false
}
func observedIdentity(st State, s Sendbox) bool {
	if exact(st.Identity, s) {
		return true
	}
	if st.Bound {
		return false
	}
	id, e := NextIdentity(s, st.Identity.Owner, st.Identity.Session)
	return e == nil && id == st.Identity
}
func (c *Coordinator) Status(ctx context.Context) (State, Sendbox, error) {
	st, e := c.Store.Load(ctx)
	if e != nil {
		return st, Sendbox{}, e
	}
	s, e := c.Backend.Observe(ctx)
	return st, s, e
}

// Connect fixes the local claim and identity. Only an accepted first command
// establishes the addon session; connection itself performs no memory write.
func (c *Coordinator) Connect(ctx context.Context, id Identity) (State, error) {
	s, e := c.Backend.Observe(ctx)
	if e != nil {
		return State{}, e
	}
	if e = validateSendbox(s); e != nil {
		return State{}, e
	}
	expected, e := NextIdentity(s, id.Owner, id.Session)
	if !exact(id, s) && (e != nil || id != expected) {
		return State{}, ErrIdentity
	}
	if !s.ControlReady {
		return State{}, ErrPending
	}
	e = c.Store.Update(ctx, func(st *State) error {
		if st.Identity != (Identity{}) && st.Identity != id {
			return ErrBusy
		}
		if st.Active != nil && !st.Active.ResultSaved {
			return ErrBusy
		}
		st.Identity = id
		st.Selected = true
		return nil
	})
	if e != nil {
		return State{}, e
	}
	return c.Store.Load(ctx)
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
	e = c.Store.Update(ctx, func(st *State) error {
		if (!st.Selected && !st.Bound) || (st.Closed || st.Closing) || !observedIdentity(*st, s) {
			return ErrIdentity
		}
		if !s.Ready || !s.TransportReady || !s.ActorReady || s.ReadyChallenge == "" || s.ReadyChallenge == zeroToken {
			return ErrBusy
		}
		if st.ReloadPrepared != nil {
			return ErrPending
		}
		if in, ok := st.Intents["stop"]; ok && (!in.Accepted || in.Message.Header.Kind == Close || in.Message.Header.Kind == Reload || in.Message.Header.Kind == Lease) {
			return ErrPending
		}
		var previous *ResultManifest
		if a := st.Active; a != nil && !a.Released {
			if !a.ResultSaved || a.Result == nil || !s.ResourcesReleased || s.Terminal == nil {
				return ErrBusy
			}
			if s.Terminal.RequestID != a.RequestID || s.Terminal.RequestSHA256 != a.Digest {
				return ErrIdentity
			}
			expected, err := ResultAckSHA256(*a.Result)
			if err != nil {
				return err
			}
			actual, err := ResultAckSHA256(*s.Terminal)
			if err != nil || actual != expected {
				return ErrIdentity
			}
			copied := *a.Result
			previous = &copied
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
		transfer := c.TransferBudgetMillis
		if transfer == 0 {
			transfer = 600000
		}
		if transfer > 600000 {
			return errors.New("transfer budget exceeds 600000 ms")
		}
		frames, err := NewFrames(st.Identity, rid, seq, 1, created, budgetMillis, source)
		if err != nil {
			return err
		}
		frames[0].Header.Challenge = s.ReadyChallenge
		frames[0].Header.PreviousResultAckSHA = zeroDigest
		if previous != nil {
			frames[0].Header.PreviousResultAckSHA, err = ResultAckSHA256(*previous)
			if err != nil {
				return err
			}
		}
		st.RequestSequence = seq
		st.Active = &ActiveRequest{RequestID: rid, Digest: frames[0].Header.RequestSHA256, Sequence: seq, Attempt: 1, Created: created, Budget: budgetMillis, TransferDeadline: created + uint64(transfer), Source: append([]byte(nil), source...), Frames: frames, Phase: "submitted", PreviousResultAck: previous}
		delete(st.Intents, "command")
		if in, ok := st.Intents["stop"]; ok && in.Accepted {
			delete(st.Intents, "stop")
		}
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
		if st.LocalRetired {
			return ErrIdentity
		}
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

// Reconcile accepts only exact private receipts, never WPM completion alone.
func (c *Coordinator) reconcile(ctx context.Context, s Sendbox) error {
	return c.Store.Update(ctx, func(st *State) error {
		if !observedIdentity(*st, s) {
			return ErrIdentity
		}
		for lane, in := range st.Intents {
			r, ok := s.Receipts[lane]
			h := in.Message.Header
			if ok && r.MessageID == h.MessageID && r.RequestID == h.RequestID && r.RequestSHA256 == h.RequestSHA256 {
				switch r.State {
				case "accepted", "closed", "closing", "cancel_requested", "not_started", "cancelled", "cancel_too_late", "repaired", "lease_observed":
					in.Accepted = true
					if lane == "stop" && s.Identity == st.Identity {
						st.Bound = true
					}
					in.Challenge = r.Challenge
				default:
					if rejectedReceipt(r.State) {
						in.Rejected = r.State
					}
				}
				st.Intents[lane] = in
			}
		}
		if a := st.Active; a != nil && !a.Released {
			if v := s.Validation; v != nil && v.RequestID == a.RequestID && v.RequestSHA256 == a.Digest {
				a.Phase = "validating"
			}
			if requestMatches(a, s.Request) {
				if s.Phase != "validating" {
					st.Bound = true
				}
				a.Phase = s.Phase
				for _, n := range s.Request.AcceptedFrames {
					if n == 1 {
						a.NextFrame = 1
						if in, ok := st.Intents["command"]; ok && in.Message.Header.RequestID == a.RequestID {
							in.Accepted = true
							st.Intents["command"] = in
						}
					}
				}
				if !s.Request.NotStarted && a.ExecutionObservedHostAt == 0 {
					a.ExecutionObservedHostAt = uint64(c.now().UnixMilli())
				}
			}
			if s.Terminal != nil && s.Terminal.RequestID == a.RequestID && s.Terminal.RequestSHA256 == a.Digest {
				st.Bound = true
			}
			if releasedMatches(a, s.Released) && s.ResourcesReleased {
				if !a.ResultSaved {
					return errors.New("release observed before host result durability")
				}
				in, ok := st.Intents["stop"]
				if !ok || in.Message.Header.Kind != Close || len(in.Message.Payload) != 92 {
					return errors.New("release without exact final ACK/close intent")
				}
				in.Accepted = true
				st.Intents["stop"] = in
				a.Released = true
				a.Phase = "released"
				a.Source = nil
				a.Frames = nil
				delete(st.Intents, "command")
			}
		}
		if in, ok := st.Intents["stop"]; ok && in.Accepted && in.Message.Header.Kind == Close && (s.Phase == "closed" || s.Phase == "ready_unbound" && s.ClosedAdmission) && s.ResourcesReleased {
			st.Closed = true
		}
		return nil
	})
}

// Step performs at most one side effect. A saved terminal remains retained in
// the addon until a later command jointly acknowledges it, or final close.
func (c *Coordinator) Step(ctx context.Context) (State, error) {
	s, e := c.Backend.Observe(ctx)
	if e != nil {
		return State{}, e
	}
	if e = validateSendbox(s); e != nil {
		return State{}, e
	}
	if e = c.reconcile(ctx, s); e != nil {
		return State{}, e
	}
	st, e := c.Store.Load(ctx)
	if e != nil {
		return st, e
	}
	if in, ok := st.Intents["stop"]; ok && in.Message.Header.Kind == Repair && !in.Accepted {
		if in.Rejected != "" {
			return st, fmt.Errorf("%w: repair %s", ErrRejected, in.Rejected)
		}
		if in.Outcome.State == NoWrite {
			return st, c.publish(ctx, in.Message)
		}
		return st, ErrPending
	}
	a := st.Active
	if a == nil || a.Released {
		return st, nil
	}
	if s.Terminal != nil && s.Terminal.RequestID == a.RequestID && s.Terminal.RequestSHA256 == a.Digest {
		if a.ResultSaved {
			return st, nil
		}
		m := *s.Terminal
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
		if e != nil {
			return st, e
		}
		return c.Store.Load(ctx)
	}
	if in, ok := st.Intents["command"]; ok {
		if in.Rejected != "" {
			return st, fmt.Errorf("%w: request %s", ErrRejected, in.Rejected)
		}
		if in.Accepted || in.Outcome.State != NoWrite {
			if !in.Accepted && a.ExecutionObservedHostAt == 0 && c.now().UnixMilli() >= int64(a.TransferDeadline) {
				return st, ErrBudget
			}
			return st, ErrPending
		}
	}
	if a.ExecutionObservedHostAt == 0 && c.now().UnixMilli() >= int64(a.TransferDeadline) {
		return st, ErrBudget
	}
	if st.Closed {
		return st, ErrPending
	}
	if in, ok := st.Intents["stop"]; ok && (in.Message.Header.RequestID == a.RequestID || in.Message.Header.Kind == Close) {
		return st, ErrPending
	}
	if e = ValidateFrameTransition(s, a.Frames[0]); e != nil {
		return st, e
	}
	return st, c.publish(ctx, a.Frames[0])
}
func (c *Coordinator) Resume(ctx context.Context) (State, error) {
	for {
		st, e := c.Step(ctx)
		if e == nil && (st.Active == nil || st.Active.ResultSaved || st.Active.Released) {
			return st, nil
		}
		if e != nil && !errors.Is(e, ErrPending) {
			return st, e
		}
		if e = c.wait(ctx); e != nil {
			durable, cancel := context.WithTimeout(context.WithoutCancel(ctx), 2*time.Second)
			latest, _ := c.Store.Load(durable)
			cancel()
			return latest, ErrPending
		}
	}
}

func (c *Coordinator) control(ctx context.Context, kind Kind, budget time.Duration) (State, error) {
	if budget <= 0 {
		return State{}, errors.New("cleanup budget must be positive")
	}
	ctx, cancel := context.WithTimeout(ctx, budget)
	defer cancel()
	if kind == Close {
		if e := c.Store.Update(ctx, func(st *State) error { st.Closing = true; return nil }); e != nil {
			return State{}, e
		}
	}
	for {
		s, e := c.Backend.Observe(ctx)
		if e != nil {
			return State{}, e
		}
		if e = validateSendbox(s); e != nil {
			return State{}, e
		}
		if e = c.reconcile(ctx, s); e != nil {
			return State{}, e
		}
		st, e := c.Store.Load(ctx)
		if e != nil {
			return st, e
		}
		if st.Closed && kind != Reload {
			return st, nil
		}
		if !s.ControlReady {
			return st, ErrPending
		}
		in, has := st.Intents["stop"]
		if has && in.Rejected != "" {
			return st, fmt.Errorf("%w: stop %s", ErrRejected, in.Rejected)
		}
		if has && !in.Accepted && in.Outcome.State != NoWrite {
			// This physical row must remain untouched until its original receipt.
		} else {
			var payload []byte
			a := st.Active
			publicationKind := kind
			if kind == Cancel && a != nil && a.PreviousResultAck != nil && !a.ResultSaved {
				payload, e = EncodeResultAck(*a.PreviousResultAck)
				if e != nil {
					return st, e
				}
			}
			if kind == Close && a != nil && a.PreviousResultAck != nil && !a.ResultSaved && !requestMatches(a, s.Request) {
				// Closing an uncertain replacement command first revokes that exact nonce.
				// Its previous-result ACK is cancellation proof, never final-close ACK.
				publicationKind = Cancel
				payload, e = EncodeResultAck(*a.PreviousResultAck)
				if e != nil {
					return st, e
				}
			}

			if (kind == Cancel || kind == Reload) && has && in.Accepted && in.Message.Header.Kind == kind {
				return st, nil
			}
			if kind == Cancel && (a == nil || a.ResultSaved) {
				return st, nil
			}
			if kind == Close && a != nil && !a.ResultSaved && s.Terminal != nil && s.Terminal.RequestID == a.RequestID && s.Terminal.RequestSHA256 == a.Digest {
				if _, e = c.Step(ctx); e != nil {
					return st, e
				}
				continue
			}
			if kind == Close && a != nil && a.ResultSaved && !a.Released {
				if a.Result == nil || !s.ResourcesReleased {
					return st, ErrPending
				}
				payload, e = EncodeResultAck(*a.Result)
				if e != nil {
					return st, e
				}
			}
			if has && in.Accepted && in.Message.Header.Kind == kind {
				if kind == Cancel {
					return st, nil
				}
				if len(in.Message.Payload) == len(payload) { // closing ACK/close must be a new exact intent only after result durability.
					if e = c.wait(ctx); e != nil {
						return st, ErrPending
					}
					continue
				}
			}
			challenge := zeroToken
			if !st.Bound && a == nil {
				challenge = s.ReadyChallenge
			}
			if a != nil && !a.Released && len(a.Frames) > 0 {
				challenge = a.Frames[0].Header.Challenge
			}
			if e = c.publishControl(ctx, publicationKind, challenge, payload); e != nil {
				return st, e
			}
		}
		if e = c.wait(ctx); e != nil {
			return st, ErrPending
		}
	}
}
func (c *Coordinator) Cancel(ctx context.Context, budget time.Duration) (State, error) {
	return c.control(ctx, Cancel, budget)
}
func (c *Coordinator) Disconnect(ctx context.Context, budget time.Duration) (State, error) {
	before, e := c.Store.Load(ctx)
	if e != nil {
		return before, e
	}
	if before.Selected && !before.Bound && before.Active == nil && len(before.Intents) == 0 && before.PublicationSequence == 0 && before.ReloadPrepared == nil {
		e = c.Store.Update(ctx, func(st *State) error {
			if st.Bound || st.Active != nil || len(st.Intents) != 0 || st.PublicationSequence != 0 || st.ReloadPrepared != nil {
				return ErrPending
			}
			st.LocalRetired = true
			st.Closing = true
			return nil
		})
		if e != nil {
			return before, e
		}
		latest, e := c.Store.Load(ctx)
		return latest, errors.Join(e, c.Backend.Close(context.WithoutCancel(ctx)))
	}
	st, e := c.control(ctx, Close, budget)
	closeCtx, cancel := context.WithTimeout(context.WithoutCancel(ctx), 2*time.Second)
	defer cancel()
	closeErr := c.Backend.Close(closeCtx)
	return st, errors.Join(e, closeErr)
}

// Reload first saves and finally acknowledges any retained terminal, then
// prepares an exact, bounded reload challenge in the same stop row.
func (c *Coordinator) Reload(ctx context.Context, budget time.Duration) (State, error) {
	if budget <= 0 {
		return State{}, errors.New("cleanup budget must be positive")
	}
	ctx, cancel := context.WithTimeout(ctx, budget)
	defer cancel()
	st, e := c.Store.Load(ctx)
	if e != nil {
		return st, e
	}
	if !st.Bound {
		return st, ErrBusy
	}
	if st.ReloadPrepared != nil {
		return st, nil
	}
	if a := st.Active; a != nil && !a.Released {
		if !a.ResultSaved {
			return st, ErrBusy
		}
		if _, e = c.control(ctx, Close, budget); e != nil {
			return st, e
		}
	}
	st, e = c.control(ctx, Reload, budget)
	if e != nil {
		return st, e
	}
	in, ok := st.Intents["stop"]
	if !ok || in.Message.Header.Kind != Reload || !in.Accepted || in.Challenge == "" || in.Challenge == zeroToken {
		return st, ErrPending
	}
	e = c.Store.Update(ctx, func(current *State) error {
		entry := current.Intents["stop"]
		if entry.Message.Header.MessageID != in.Message.Header.MessageID || !entry.Accepted {
			return ErrIdentity
		}
		copy := entry
		current.ReloadPrepared = &copy
		current.Closed = false
		return nil
	})
	if e != nil {
		return st, e
	}
	return c.Store.Load(ctx)
}

// CommitReload never republishes an uncertain lease. Its original preparation
// remains durable separately from the reused stop row.
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
	prepared := st.ReloadPrepared
	if prepared == nil || !prepared.Accepted || prepared.Challenge == "" || st.Active != nil && !st.Active.Released {
		return st, ErrPending
	}
	payload, e := token(prepared.Message.Header.MessageID)
	if e != nil {
		return st, e
	}
	old, has := st.Intents["stop"]
	if has && old.Message.Header.Kind == Lease {
		if hex.EncodeToString(old.Message.Payload) != prepared.Message.Header.MessageID || old.Message.Header.Challenge != prepared.Challenge {
			return st, ErrIdentity
		}
		if old.Accepted {
			return st, nil
		}
		if old.Outcome.State == NoWrite {
			e = c.publish(ctx, old.Message)
			if e != nil {
				return st, e
			}
		}
	} else {
		if has && !old.Accepted && old.Outcome.State != NoWrite {
			return st, ErrPending
		}
		if e = c.publishControl(ctx, Lease, prepared.Challenge, payload); e != nil {
			return st, e
		}
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
		in := st.Intents["stop"]
		if in.Accepted && in.Message.Header.Kind == Lease {
			return st, nil
		}
		if in.Rejected != "" {
			return st, fmt.Errorf("%w: reload lease %s", ErrRejected, in.Rejected)
		}
		if e = c.wait(ctx); e != nil {
			return st, ErrPending
		}
	}
}

// Repair switches only a proven idle arena. Unknown business is retained and
// never automatically resubmitted into the replacement allocation.
func (c *Coordinator) Repair(ctx context.Context, drained bool) (State, error) {
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
	p := s.Repair
	if !st.Bound {
		return st, ErrBusy
	}
	if !drained || p == nil || !p.LedgerRetained || !p.Idle || !p.NoPendingRequest || !p.ResourcesReleased || p.NotStarted || p.PreviousArena != st.Identity.Arena || p.NewArena != s.Arena || p.PreviousArena == p.NewArena || s.Runtime != st.Identity.Runtime || s.ActorBinding != st.Identity.ActorBinding || s.Request != nil || s.Terminal != nil || !s.ResourcesReleased || st.Active != nil && !st.Active.Released {
		return st, ErrUnknown
	}
	id := st.Identity
	id.Arena = s.Arena
	probe := st
	probe.Identity = id
	if !observedIdentity(probe, s) {
		return st, ErrIdentity
	}
	if in, ok := st.Intents["stop"]; ok && !in.Accepted && in.Outcome.State != NoWrite {
		return st, ErrPending
	}
	mid, e := NewToken()
	if e != nil {
		return st, e
	}
	h := Header{Kind: Repair, Runtime: id.Runtime, Arena: id.Arena, Session: id.Session, Owner: id.Owner, ActorBinding: id.ActorBinding, Fence: id.Fence, MessageID: mid, RequestID: zeroToken, RequestSHA256: zeroDigest, Challenge: p.Challenge, TransportAttempt: 1, CreatedUTCMillis: uint64(c.now().UnixMilli())}
	e = c.publish(ctx, Message{Header: h}, func(current *State) error {
		if current.Identity != st.Identity || current.Active != nil && !current.Active.Released {
			return ErrIdentity
		}
		proof := *p
		current.RepairProof = &proof
		current.Identity = id
		return nil
	})
	if e != nil {
		return st, e
	}
	return c.Store.Load(ctx)
}
