package protocol_test

import (
	"github.com/follenfang/lycheedev/internal/testkit"
	"testing"
)

func luaRuntime(t *testing.T) string { t.Helper(); return testkit.Lua51(t) }
