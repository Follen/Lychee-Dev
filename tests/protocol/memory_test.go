package protocol_test

import (
	"context"
	"os/exec"
	"path/filepath"
	"testing"
	"time"
)

func TestMailboxRuntimeIdleMemory(t *testing.T) {
	lua := luaRuntime(t)
	root, err := filepath.Abs("../../addon")
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), time.Minute)
	defer cancel()
	output, err := exec.CommandContext(ctx, lua, "memory.lua", root, wowGlobals(t)).CombinedOutput()
	t.Logf("stock Lua 5.1 allocation regression (WoW's 24-byte TValue has a different fixed cost):\n%s", output)
	if err != nil {
		t.Fatalf("actual addon startup/Runtime.OnUpdate memory budgets: %v", err)
	}
}
