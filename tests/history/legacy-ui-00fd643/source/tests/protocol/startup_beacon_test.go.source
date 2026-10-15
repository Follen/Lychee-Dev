package protocol_test

import (
	"os/exec"
	"testing"
)

func TestLuaStartupBeacon(t *testing.T) {
	if out, err := exec.Command(luaRuntime(t), "startup_beacon.lua", "../../addon").CombinedOutput(); err != nil {
		t.Fatalf("%v\n%s", err, out)
	}
}
