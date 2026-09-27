package protocol_test

import (
	"os/exec"
	"strings"
	"testing"
)

func TestLuaReceiverBindings(t *testing.T) {
	output, err := exec.Command(luaRuntime(t), "receiver_bindings.lua", "../../addon").CombinedOutput()
	if err != nil {
		t.Fatalf("receiver bindings Lua test: %v\n%s", err, output)
	}
	if !strings.Contains(string(output), "receiver bindings: conflict, rollback and effective profile") {
		t.Fatalf("receiver bindings Lua test did not complete: %s", output)
	}
}
