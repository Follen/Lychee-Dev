package protocol_test

import (
	"os/exec"
	"strings"
	"testing"
)

func TestInputStateObservation(t *testing.T) {
	output, err := exec.Command(luaRuntime(t), "input_state.lua", "../../addon").CombinedOutput()
	if err != nil || !strings.Contains(string(output), "input state: four profiles passed") {
		t.Fatalf("%v\n%s", err, output)
	}
}

func TestSessionInputWait(t *testing.T) {
	output, err := exec.Command(luaRuntime(t), "input_wait.lua", "../../addon").CombinedOutput()
	if err != nil || !strings.Contains(string(output), "input wait: passed") {
		t.Fatalf("%v\n%s", err, output)
	}
}
