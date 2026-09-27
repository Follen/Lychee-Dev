package protocol_test

import (
	"os/exec"
	"strings"
	"testing"
)

func TestLuaReceiverCorrelationAndInputFaults(t *testing.T) {
	output, err := exec.Command(luaRuntime(t), "receiver.lua", "../../addon", "../../protocol/receiver/golden.json").CombinedOutput()
	if err != nil {
		t.Fatalf("receiver Lua test: %v\n%s", err, output)
	}
	if !strings.Contains(string(output), "receiver: incremental framing") {
		t.Fatalf("receiver Lua test did not complete: %s", output)
	}
}

func TestLuaReceiverRollsBackPartialUI(t *testing.T) {
	output, err := exec.Command(luaRuntime(t), "receiver_rollback.lua", "../../addon").CombinedOutput()
	if err != nil {
		t.Fatalf("receiver rollback Lua test: %v\n%s", err, output)
	}
	if !strings.Contains(string(output), "receiver rollback ok") {
		t.Fatalf("receiver rollback Lua test did not complete: %s", output)
	}
}
