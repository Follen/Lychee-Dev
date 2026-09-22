package protocol_test

import (
	"fmt"
	"os"
	"os/exec"
	"regexp"
	"testing"
)

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
