package duplex

import (
	"context"
	"encoding/json"
	"errors"
	"math"
	"os"
	"path/filepath"
	"testing"
	"time"
)

func closedFixture(t *testing.T) (*Coordinator, *fixtureBackend, *fixtureStore, Identity) {
	t.Helper()
	c, b, oldStore := newFixtureCoordinator(t)
	ctx := context.Background()
	st, e := c.Execute(ctx, []byte("return 42"), 1000)
	if e != nil || !st.Active.Released || !st.Active.ResultSaved {
		t.Fatal("first result was not durable and acknowledged", e)
	}
	if _, e = c.Disconnect(ctx, time.Second); e != nil {
		t.Fatal(e)
	}
	id, e := NextIdentity(b.box, "abababababababababababababababab", "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd")
	if e != nil {
		t.Fatal(e)
	}
	return c, b, oldStore, id
}

func TestCloseFreshBindExecuteAndOldOwnerDenied(t *testing.T) {
	old, b, oldStore, id := closedFixture(t)
	firstRequest := b.box.Released.RequestID
	newStore := newFixtureStore()
	b.store = newStore
	c := NewCoordinator(b, newStore)
	c.PollInterval = time.Millisecond
	ctx := context.Background()
	if st, e := c.Connect(ctx, id); e != nil || !st.Bound || st.Closed {
		t.Fatal("fresh bind failed", e)
	}
	in := newStore.state.Intents["bindResume"]
	if len(in.Message.Payload) != ClosedBindProofBytes || id.Fence != fixtureIdentity.Fence+1 || b.box.Released.RequestID != firstRequest {
		t.Fatal("new session lost its fenced close proof or released tombstone")
	}
	st, e := c.Execute(ctx, []byte("return 43"), 1000)
	if e != nil || !st.Active.Released || st.Active.Sequence != 1 || st.Active.RequestID == firstRequest || b.executions != 2 {
		t.Fatal("second session did not execute exactly once", e)
	}
	if e = ValidateState(st); e != nil {
		t.Fatal(e)
	}
	writes := len(b.writes)
	b.store = oldStore
	if _, e = old.Cancel(ctx, time.Second); !errors.Is(e, ErrIdentity) {
		t.Fatal("retired owner cancellation was admitted", e)
	}
	if _, e = old.Execute(ctx, []byte("return 99"), 1000); !errors.Is(e, ErrIdentity) {
		t.Fatal("retired owner business was admitted", e)
	}
	if len(b.writes) != writes || b.executions != 2 {
		t.Fatal("retired owner produced an effect")
	}
}

func TestFreshBindProofAdmissionBoundaries(t *testing.T) {
	_, b, _, id := closedFixture(t)
	proof, e := EncodeClosedBindProof(b.box)
	if e != nil {
		t.Fatal(e)
	}
	h := Header{Kind: Bind, Runtime: id.Runtime, Arena: id.Arena, Session: id.Session, Owner: id.Owner, ActorBinding: id.ActorBinding, Fence: id.Fence, RequestID: "00000000000000000000000000000000", RequestSHA256: "0000000000000000000000000000000000000000000000000000000000000000"}
	if e = ValidateBindTransition(b.box, Message{Header: h, Payload: proof}); e != nil {
		t.Fatal(e)
	}
	for _, offset := range []int{0, 16, 32, 40, 56, 87} {
		bad := append([]byte(nil), proof...)
		bad[offset] ^= 1
		if ValidateBindTransition(b.box, Message{Header: h, Payload: bad}) == nil {
			t.Fatalf("accepted forged prior close proof at %d", offset)
		}
	}
	for _, mutate := range []func(*Sendbox){
		func(s *Sendbox) { s.Phase = "idle" },
		func(s *Sendbox) { s.ResourcesReleased = false },
		func(s *Sendbox) { s.Request = &RequestState{} },
		func(s *Sendbox) { s.Terminal = &ResultManifest{} },
		func(s *Sendbox) { s.Fence = math.MaxUint64 },
	} {
		s := b.box
		mutate(&s)
		if _, e = NextIdentity(s, id.Owner, id.Session); e == nil {
			t.Fatal("admitted ownership transition without private quiescence")
		}
	}
	s := b.box
	s.Fence = 9007199254740993
	next, e := NextIdentity(s, id.Owner, id.Session)
	if e != nil || next.Fence != 9007199254740994 {
		t.Fatal("fresh owner fence rounded", e)
	}
}

func TestFreshBindInterruptedOutcomeNeverReplays(t *testing.T) {
	_, b, _, id := closedFixture(t)
	s := newFixtureStore()
	b.store = s
	b.stalled = true
	b.unknown = Bind
	c := NewCoordinator(b, s)
	c.PollInterval = time.Millisecond
	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Millisecond)
	defer cancel()
	if _, e := c.Connect(ctx, id); e == nil || s.state.Intents["bindResume"].Outcome.State != UnknownWrite {
		t.Fatal("lost bind outcome was not retained", e)
	}
	writes := len(b.writes)
	b.stalled = false
	if _, e := c.Step(context.Background()); !errors.Is(e, ErrPending) || len(b.writes) != writes {
		t.Fatal("unknown bind was replayed", e)
	}
	in := s.state.Intents["bindResume"]
	b.box.Identity = id
	b.box.Phase = "idle"
	b.box.BusinessReady = true
	b.receipt(in.Message, "bound")
	st, e := c.Step(context.Background())
	if e != nil || !st.Bound || len(b.writes) != writes {
		t.Fatal("original bind receipt did not recover", e)
	}
}

func TestFreshBindProvenNoWriteCanResume(t *testing.T) {
	_, b, _, id := closedFixture(t)
	s := newFixtureStore()
	b.store = s
	b.bindNoWrite = true
	c := NewCoordinator(b, s)
	c.PollInterval = time.Millisecond
	if _, e := c.Connect(context.Background(), id); e == nil {
		t.Fatal("failed bind had no error")
	}
	original := s.state.Intents["bindResume"].Message.Header.MessageID
	b.bindNoWrite = false
	st, e := c.Resume(context.Background())
	if e != nil || !st.Bound || st.Intents["bindResume"].Message.Header.MessageID != original {
		t.Fatal("proven no-write bind could not resume original intent", e)
	}
}

func TestRejectedFreshBindDoesNotRemainPending(t *testing.T) {
	_, b, _, id := closedFixture(t)
	s := newFixtureStore()
	b.store = s
	b.reject = Bind
	c := NewCoordinator(b, s)
	c.PollInterval = time.Millisecond
	if _, e := c.Connect(context.Background(), id); !errors.Is(e, ErrRejected) {
		t.Fatal("exact bind rejection was hidden by old owner projection", e)
	}
	if s.state.Intents["bindResume"].Rejected != "challenge_expired" || s.state.Bound || b.box.Identity != fixtureIdentity {
		t.Fatal("rejected bind changed ownership")
	}
}

func TestValidateStateRejectsCorruptDurableFacts(t *testing.T) {
	c, _, _ := newFixtureCoordinator(t)
	st, e := c.Execute(context.Background(), []byte("return 42"), 1000)
	if e != nil {
		t.Fatal(e)
	}
	for _, mutate := range []func(*State){
		func(s *State) { s.Identity = Identity{} },
		func(s *State) { s.Identity.Session = "bad" },
		func(s *State) { s.Bound = false },
		func(s *State) { s.Active.Digest = "bad" },
		func(s *State) { s.Active.Result.RequestID = fixtureIdentity.Owner },
		func(s *State) { s.Active.Source = []byte("retained after release") },
		func(s *State) {
			in := s.Intents["bindResume"]
			in.Outcome.State = "invented"
			s.Intents["bindResume"] = in
		},
		func(s *State) {
			in := s.Intents["bindResume"]
			in.Rejected = "owner_conflict"
			s.Intents["bindResume"] = in
		},
	} {
		bad := copyState(st)
		mutate(&bad)
		if ValidateState(bad) == nil {
			t.Fatal("corrupt durable facts accepted")
		}
	}
	st.Identity = Identity{}
	p, e := json.Marshal(st)
	if e != nil {
		t.Fatal(e)
	}
	dir := t.TempDir()
	if e = os.WriteFile(filepath.Join(dir, "state.json"), p, 0600); e != nil {
		t.Fatal(e)
	}
	if _, e = NewFileStore(dir).Load(context.Background()); e == nil {
		t.Fatal("file store read bypassed durable validation")
	}
}
