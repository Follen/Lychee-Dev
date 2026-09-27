package inputlab_test

import (
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
)

func TestLuaReceiver(t *testing.T) {
	name := os.Getenv("LYCHEEDEV_LUA51")
	if name == "" {
		name = "lua"
	}
	exe, err := exec.LookPath(name)
	if err != nil {
		if os.Getenv("LYCHEEDEV_REQUIRE_LUA51") == "1" {
			t.Fatal(err)
		}
		t.Skip("Lua 5.1 unavailable")
	}
	out, err := exec.Command(exe, "-v").CombinedOutput()
	if err != nil || !strings.HasPrefix(string(out), "Lua 5.1.") {
		t.Fatalf("Lua 5.1 required: %s %v", out, err)
	}
	for _, name := range []string{"offline.lua", "addon/Protocol.lua", "addon/Lab.lua"} {
		if _, err = os.ReadFile(name); err != nil {
			t.Fatal(err)
		}
	}
	out, err = exec.Command(exe, "offline.lua", filepath.Join("addon", "Protocol.lua"), filepath.Join("addon", "Lab.lua")).CombinedOutput()
	if err != nil {
		t.Fatalf("%v\n%s", err, out)
	}
	t.Log(string(out))
}
