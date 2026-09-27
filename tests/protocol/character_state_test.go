package protocol_test

import (
	"os/exec"
	"testing"
)

func TestCharacterBridgeStorageDoesNotImportAccountExecution(t *testing.T) {
	output, err := exec.Command(luaRuntime(t), "character_state.lua", "../../addon").CombinedOutput()
	if err != nil {
		t.Fatalf("%v\n%s", err, output)
	}
}
