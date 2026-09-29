// Test-only Lua peer. Keep the interpreter policy aligned with tests/protocol.
package slotset

import (
	"context"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"testing"
	"time"
)

func TestLuaCollectionLoaderPrototype(t *testing.T) {
	names := []string{"lua5.1", "lua51", "lua"}
	explicit := os.Getenv("LYCHEEDEV_LUA51")
	if explicit != "" {
		names = []string{explicit}
	}
	var lua string
	for _, name := range names {
		path, err := exec.LookPath(name)
		if err != nil {
			continue
		}
		output, err := exec.Command(path, "-v").CombinedOutput()
		if err == nil && regexp.MustCompile(`^Lua 5\.1\.[0-9]+(?:\s|$)`).Match(output) {
			lua = path
			break
		}
	}
	if lua == "" {
		if explicit != "" || os.Getenv("LYCHEEDEV_REQUIRE_LUA51") == "1" {
			t.Fatal("required Lua 5.1 unavailable")
		}
		t.Skip("optional local Lua 5.1 unavailable")
	}
	root, err := filepath.Abs("../..")
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()
	output, err := exec.CommandContext(ctx, lua, "loader_prototype.lua", filepath.ToSlash(root)).CombinedOutput()
	t.Log(string(output))
	if err != nil {
		t.Fatalf("Lua collection prototype: %v", err)
	}
}
