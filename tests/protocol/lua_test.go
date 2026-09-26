package protocol_test

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"runtime"
	"testing"
)

// wowGlobals returns the absolute path of the client-global shim every protocol
// fixture loads first. A plain Lua 5.1 interpreter does not expose the client's
// short library aliases (strmatch and friends), and addon or vendored source may
// call them, so the shim must be installed before any addon file is loaded.
func wowGlobals(t *testing.T) string {
	t.Helper()
	_, thisFile, _, ok := runtime.Caller(0)
	if !ok {
		t.Fatal("runtime.Caller failed")
	}
	return filepath.Join(filepath.Dir(thisFile), "..", "lib", "wow_globals.lua")
}

func luaRuntime(t *testing.T) string {
	t.Helper()
	names := []string{"lua5.1", "lua51", "lua"}
	explicit := os.Getenv("LYCHEEDEV_LUA51")
	if explicit != "" {
		names = []string{explicit}
	}
	var failures []string
	for _, name := range names {
		path, err := exec.LookPath(name)
		if err != nil {
			failures = append(failures, err.Error())
			continue
		}
		output, err := exec.Command(path, "-v").CombinedOutput()
		if err == nil && regexp.MustCompile(`^Lua 5\.1\.[0-9]+(?:\s|$)`).Match(output) {
			return path
		}
		failures = append(failures, fmt.Sprintf("%s: %v: %s", path, err, output))
	}
	if explicit != "" || os.Getenv("LYCHEEDEV_REQUIRE_LUA51") == "1" {
		t.Fatalf("required Lua 5.1 unavailable: %v", failures)
	}
	t.Skipf("optional local Lua 5.1 unavailable: %v", failures)
	return ""
}
