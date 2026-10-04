package duplex

import (
	"strings"
	"testing"
)

func TestSingleRowRequiresExactPrivateFrameAcknowledgement(t *testing.T) {
	frames, err := NewFrames(fixtureIdentity, strings.Repeat("6", 32), 3, 2, 1234, 1000, make([]byte, 8192))
	if err != nil {
		t.Fatal(err)
	}
	s := fixtureSendbox()
	if err := ValidateFrameTransition(s, frames[0]); err != nil {
		t.Fatal(err)
	}
	if err := ValidateFrameTransition(s, frames[1]); err == nil {
		t.Fatal("second frame overwrote unacknowledged first frame")
	}
	h := frames[0].Header
	s.Phase = "receiving"
	s.BusinessReady, s.Ready = false, false
	s.Request = &RequestState{RequestID: h.RequestID, RequestSHA256: h.RequestSHA256, RequestSeq: h.RequestSeq, TransportAttempt: h.TransportAttempt, NotStarted: true, AcceptedFrames: []uint32{1}}
	if err := ValidateFrameTransition(s, frames[1]); err != nil {
		t.Fatal(err)
	}
	if err := ValidateFrameTransition(s, frames[0]); err == nil {
		t.Fatal("old logical frame permitted reuse")
	}
	s.Request.AcceptedFrames = []uint32{2}
	if err := ValidateFrameTransition(s, frames[1]); err == nil {
		t.Fatal("noncontiguous acknowledgement permitted reuse")
	}
	s.Request.AcceptedFrames = []uint32{1}
	s.Request.TransportAttempt++
	if err := ValidateFrameTransition(s, frames[1]); err == nil {
		t.Fatal("old repair attempt permitted reuse")
	}
	s.Request.TransportAttempt--
	for _, phase := range []string{"prepared", "running", "execution_unknown", "result_pending", "closed"} {
		s.Phase = phase
		if err := ValidateFrameTransition(s, frames[1]); err == nil {
			t.Fatalf("reuse allowed during %s", phase)
		}
	}
}
