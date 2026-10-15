package protocol_test

import (
	"bytes"
	"os/exec"
	"testing"
)

func TestFourClientLifecycle(t *testing.T) {
	lua := luaRuntime(t)
	output, err := exec.Command(lua, "lifecycle.lua", "../../addon", wowGlobals(t)).CombinedOutput()
	if err != nil {
		t.Fatalf("%v\n%s", err, output)
	}
	if !bytes.Contains(output, []byte("lifecycle: four clients passed")) {
		t.Fatalf("missing lifecycle result: %s", output)
	}
}
