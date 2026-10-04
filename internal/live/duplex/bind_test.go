package duplex

import (
	"context"
	"errors"
	"strings"
	"testing"
	"time"
)

func TestCompetingFirstOwnerCannotConsumeAcceptedChallenge(t *testing.T) {
	c, b, _ := newFixtureCoordinator(t)
	second := NewCoordinator(b, newFixtureStore())
	id := fixtureIdentity
	id.Owner = strings.Repeat("b", 32)
	id.Session = strings.Repeat("c", 32)
	if _, e := second.Connect(context.Background(), id); e != nil {
		t.Fatal(e)
	}
	if _, e := c.Execute(context.Background(), []byte("return42"), 1000); e != nil {
		t.Fatal(e)
	}
	before := len(b.writes)
	if _, e := second.Execute(context.Background(), []byte("return99"), 1000); !errors.Is(e, ErrIdentity) {
		t.Fatal(e)
	}
	if len(b.writes) != before {
		t.Fatal("competing owner wrote")
	}
	if _, e := c.Disconnect(context.Background(), time.Second); e != nil {
		t.Fatal(e)
	}
}

func TestClosedAdmissionRebindsWithoutReload(t *testing.T) {
	old, b, oldStore := newFixtureCoordinator(t)
	if _, e := old.Execute(context.Background(), []byte("return42"), 1000); e != nil {
		t.Fatal(e)
	}
	if _, e := old.Disconnect(context.Background(), time.Second); e != nil {
		t.Fatal(e)
	}
	b.box.Phase = "ready_unbound"
	b.box.ClosedAdmission = true
	b.box.Ready = true
	b.box.BusinessReady = true
	b.box.ReadyChallenge = strings.Repeat("e", 32)
	b.box.AdmissionSequence++
	id, e := NextIdentity(b.box, strings.Repeat("b", 32), strings.Repeat("c", 32))
	if e != nil {
		t.Fatal(e)
	}
	newStore := newFixtureStore()
	b.store = newStore
	next := NewCoordinator(b, newStore)
	if _, e = next.Connect(context.Background(), id); e != nil {
		t.Fatal(e)
	}
	st, e := next.Execute(context.Background(), []byte("return43"), 1000)
	if e != nil || st.Active.Sequence != 1 || id.Fence != fixtureIdentity.Fence+1 || b.executions != 2 {
		t.Fatal("same-runtime rebind failed", e)
	}
	b.store = oldStore
	before := len(b.writes)
	if _, e = old.Cancel(context.Background(), time.Second); !errors.Is(e, ErrIdentity) {
		t.Fatal("retired owner control admitted", e)
	}
	if len(b.writes) != before {
		t.Fatal("retired owner wrote")
	}
}
