package bridge

import (
	"encoding/json"
	"strings"
	"testing"
)

func receiverSignalFixture(kind string) Signal {
	s := Signal{Schema: "lycheedev.signal.v1", Release: "2.0.6", Kind: kind,
		Product: "retail", Build: "12.1.0.69933", Sequence: 2, RuntimeEpoch: 9,
		ReceiverNonce: strings.Repeat("a", 32), InputReady: true}
	if kind == "receiver_ready" {
		s.WakeBinding, s.SubmitBinding, s.CloseBinding = "ALT-CTRL-]", "ALT-CTRL-SHIFT-]", "ALT-CTRL-["
	}
	if kind != "receiver_ready" {
		s.RequestID, s.AttemptID = "REQ_one", strings.Repeat("b", 16)
		s.BodyBytes, s.BodyAdler32 = uint32(len("run|REQ_one|-")), "1c5304da"
	}
	if kind == "receiver_commit_ready" {
		s.CommitNonce = strings.Repeat("c", 16)
	}
	if kind == "receiver_accepted" {
		s.Accepted, s.InputReady = true, false
	}
	if kind == "receiver_rejected" {
		s.InputReady, s.ErrorCode = false, "receiver_checksum"
		s.RequestID, s.AttemptID, s.BodyBytes, s.BodyAdler32 = "", "", 0, ""
	}
	return s
}

func TestReceiverSignalValidationAndCorrelation(t *testing.T) {
	for _, kind := range []string{"receiver_ready", "receiver_staged", "receiver_commit_ready", "receiver_accepted", "receiver_rejected"} {
		s := receiverSignalFixture(kind)
		data, _ := json.Marshal(s)
		got, err := ParseSignal(data)
		if err != nil || got.Kind != kind {
			t.Fatalf("%s: %+v %v", kind, got, err)
		}
		if kind == "receiver_ready" {
			continue
		}
		e := SignalExpectation{Release: s.Release, Kind: kind, Product: s.Product, Build: s.Build, ReceiverNonce: s.ReceiverNonce, RuntimeEpoch: s.RuntimeEpoch, AfterSequence: 1}
		if kind != "receiver_rejected" {
			e.RequestID, e.AttemptID, e.BodyBytes, e.BodyAdler32 = s.RequestID, s.AttemptID, s.BodyBytes, s.BodyAdler32
		}
		if err := got.Match(e); err != nil {
			t.Fatalf("%s match: %v", kind, err)
		}
		e.AttemptID = strings.Repeat("d", 16)
		if kind != "receiver_rejected" && got.Match(e) == nil {
			t.Fatalf("%s matched foreign attempt", kind)
		}
	}
	staged := receiverSignalFixture("receiver_staged")
	mutations := []func(*Signal){
		func(s *Signal) { s.ReceiverNonce = strings.Repeat("A", 32) },
		func(s *Signal) { s.BodyBytes = 0 },
		func(s *Signal) { s.CommitNonce = strings.Repeat("c", 16) },
		func(s *Signal) { s.Accepted = true },
		func(s *Signal) { s.RuntimeEpoch = 0 },
		func(s *Signal) { s.SessionNonce = strings.Repeat("d", 32) },
		func(s *Signal) { s.WakeBinding = "ALT-CTRL-F1" },
	}
	for i, mutate := range mutations {
		s := staged
		mutate(&s)
		data, _ := json.Marshal(s)
		if _, err := ParseSignal(data); err == nil {
			t.Fatalf("accepted mutation %d", i)
		}
	}
	ready := receiverSignalFixture("receiver_ready")
	ready.SessionNonce = strings.Repeat("e", 32)
	ready.PriorSessionSequence, ready.PriorSessionEpoch = 4, ready.RuntimeEpoch
	if raw, err := json.Marshal(ready); err != nil {
		t.Fatal(err)
	} else if parsed, err := ParseSignal(raw); err != nil || parsed.SessionNonce != ready.SessionNonce || parsed.PriorSessionSequence != 4 || parsed.PriorSessionEpoch != ready.RuntimeEpoch {
		t.Fatalf("existing session receiver ready: %+v, %v", parsed, err)
	}
	wrongEpoch := ready
	wrongEpoch.PriorSessionEpoch++
	if raw, err := json.Marshal(wrongEpoch); err != nil {
		t.Fatal(err)
	} else if _, err := ParseSignal(raw); err == nil {
		t.Fatal("accepted another runtime's prior session epoch")
	}
	stagedWithPrior := receiverSignalFixture("receiver_staged")
	stagedWithPrior.PriorSessionSequence = 4
	if raw, err := json.Marshal(stagedWithPrior); err != nil {
		t.Fatal(err)
	} else if _, err := ParseSignal(raw); err == nil {
		t.Fatal("accepted prior session fields outside receiver ready")
	}
	ready.SessionNonce, ready.PriorSessionSequence, ready.PriorSessionEpoch = "", 0, 0
	for i, mutate := range []func(*Signal){
		func(s *Signal) { s.WakeBinding = "" },
		func(s *Signal) { s.CloseBinding = "ALT-CTRL-SHIFT-]" },
		func(s *Signal) { s.SubmitBinding = "ALT-CTRL-A" },
	} {
		s := ready
		mutate(&s)
		data, _ := json.Marshal(s)
		if _, err := ParseSignal(data); err == nil {
			t.Fatalf("accepted binding mutation %d", i)
		}
	}
}
