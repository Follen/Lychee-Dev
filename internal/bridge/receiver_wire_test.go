package bridge

import (
	"encoding/json"
	"os"
	"strings"
	"testing"
)

func TestReceiverGoldenActions(t *testing.T) {
	data, err := os.ReadFile("../../protocol/receiver/golden.json")
	if err != nil {
		t.Fatal(err)
	}
	var cases []struct {
		Name, RequestID, Action, Arg string
		Valid                        bool
	}
	if err := json.Unmarshal(data, &cases); err != nil {
		t.Fatal(err)
	}
	for _, tc := range cases {
		t.Run(tc.Name, func(t *testing.T) {
			stage := ReceiverStage{ReceiverNonce: strings.Repeat("a", 32), RuntimeEpoch: 42, RequestID: tc.RequestID, AttemptID: strings.Repeat("b", 16), Action: tc.Action, Arg: tc.Arg}
			wire, err := EncodeReceiverStage(stage)
			if (err == nil) != tc.Valid {
				t.Fatalf("valid=%v wire=%q err=%v", tc.Valid, wire, err)
			}
			if err != nil {
				return
			}
			got, err := ParseReceiverStage(wire)
			if err != nil || got != stage {
				t.Fatalf("roundtrip %+v %v", got, err)
			}
		})
	}
}

func TestReceiverWireRejectsPollutionAndOldCommit(t *testing.T) {
	stage := ReceiverStage{ReceiverNonce: strings.Repeat("a", 32), RuntimeEpoch: 7, RequestID: "REQ_one", AttemptID: strings.Repeat("b", 16), Action: "run", Arg: "-"}
	wire, err := EncodeReceiverStage(stage)
	if err != nil {
		t.Fatal(err)
	}
	for _, polluted := range []string{wire + "x", "x" + wire, wire[:len(wire)-1], strings.Replace(wire, "REQ_one", "REQ_two", 1), strings.Replace(wire, ":7:", ":07:", 1), strings.Replace(wire, ":7:", ":8:", 1)} {
		if _, err := ParseReceiverStage(polluted); err == nil {
			t.Fatalf("accepted polluted stage %q", polluted)
		}
	}
	digest, _ := stage.BodyAdler32()
	old := ReceiverCommit{ReceiverNonce: stage.ReceiverNonce, AttemptID: stage.AttemptID, CommitNonce: strings.Repeat("c", 16), BodyAdler32: digest}
	commit, err := EncodeReceiverCommit(old)
	if err != nil {
		t.Fatal(err)
	}
	if parsed, err := ParseReceiverCommit(commit); err != nil || parsed != old {
		t.Fatalf("commit roundtrip %+v %v", parsed, err)
	}
	for _, polluted := range []string{commit + "x", commit[:len(commit)-1], strings.Replace(commit, old.AttemptID, strings.Repeat("d", 16), 1), strings.Replace(commit, old.CommitNonce, strings.Repeat("e", 16), 1)} {
		if _, err := ParseReceiverCommit(polluted); err == nil {
			t.Fatalf("accepted polluted commit %q", polluted)
		}
	}
	newStage := stage
	newStage.AttemptID = strings.Repeat("d", 16)
	newChallenge := Signal{Kind: "receiver_commit_ready", ReceiverNonce: newStage.ReceiverNonce, RuntimeEpoch: newStage.RuntimeEpoch, RequestID: newStage.RequestID, AttemptID: newStage.AttemptID, BodyBytes: uint32(len("run|REQ_one|-")), BodyAdler32: digest, CommitNonce: strings.Repeat("e", 16), InputReady: true}
	if _, err := CommitForReceiverChallenge(newStage, newChallenge); err != nil {
		t.Fatal(err)
	}
	if old.AttemptID == newChallenge.AttemptID || old.CommitNonce == newChallenge.CommitNonce {
		t.Fatal("old commit unexpectedly matches new challenge")
	}
	if _, err := CommitForReceiverChallenge(stage, newChallenge); err == nil {
		t.Fatal("late old attempt matched new challenge")
	}
}

func TestHostReceiverStageBudgetPreflight(t *testing.T) {
	stage := ReceiverStage{ReceiverNonce: strings.Repeat("a", 32), RuntimeEpoch: 9007199254740990,
		RequestID: strings.Repeat("R", 80), AttemptID: strings.Repeat("b", 16), Action: "prepare", Arg: strings.Repeat("c", 32)}
	if _, err := EncodeReceiverStage(stage); err == nil {
		t.Fatal("oversized but parser-legal stage bypassed host timing limit")
	}
	stage.RequestID = "REQ_" + strings.Repeat("d", 32)
	wire, err := EncodeReceiverStage(stage)
	if err != nil || len(wire) > MaxHostReceiverStageBytes {
		t.Fatalf("normal request excluded: %d %v", len(wire), err)
	}
}
