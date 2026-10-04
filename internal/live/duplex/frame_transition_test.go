package duplex

import (
	"strings"
	"testing"
)

func TestCommandRequiresFreshChallengeAndExactJointACK(t *testing.T) {
	frames, _ := NewFrames(fixtureIdentity, strings.Repeat("6", 32), 1, 1, 1234, 1000, []byte("return42"))
	m := frames[0]
	s := fixtureSendbox()
	m.Header.Challenge = s.ReadyChallenge
	m.Header.PreviousResultAckSHA = zeroDigest
	if e := ValidateFrameTransition(s, m); e != nil {
		t.Fatal(e)
	}
	for _, mutate := range []func(*Sendbox){func(s *Sendbox) { s.ReadyChallenge = strings.Repeat("b", 32) }, func(s *Sendbox) { s.Phase = "running" }, func(s *Sendbox) { s.ActorReady = false }, func(s *Sendbox) { s.Owner = strings.Repeat("c", 32) }} {
		bad := s
		mutate(&bad)
		if ValidateFrameTransition(bad, m) == nil {
			t.Fatal("invalid admission permitted")
		}
	}
	c, b, _ := newFixtureCoordinator(t)
	st, e := c.Execute(t.Context(), []byte("return42"), 1000)
	if e != nil {
		t.Fatal(e)
	}
	frames, _ = NewFrames(fixtureIdentity, strings.Repeat("7", 32), 2, 1, 1235, 1000, []byte("return43"))
	m = frames[0]
	m.Header.Challenge = b.box.ReadyChallenge
	m.Header.PreviousResultAckSHA, _ = ResultAckSHA256(*st.Active.Result)
	if e = ValidateFrameTransition(b.box, m); e != nil {
		t.Fatal(e)
	}
	m.Header.PreviousResultAckSHA = zeroDigest
	if ValidateFrameTransition(b.box, m) == nil {
		t.Fatal("jointACK mismatch accepted")
	}
}
