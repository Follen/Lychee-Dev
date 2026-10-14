package protocol_test

import (
	"github.com/follenfang/lycheedev/internal/testkit"
	"path/filepath"
	"runtime"
	"testing"
)

func wowGlobals(t *testing.T) string {
	t.Helper()
	_, file, _, ok := runtime.Caller(0)
	if !ok {
		t.Fatal("runtime.Caller failed")
	}
	return filepath.Join(filepath.Dir(file), "..", "lib", "wow_globals.lua")
}
func luaRuntime(t *testing.T) string { t.Helper(); return testkit.Lua51(t) }
