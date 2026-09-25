package protocol_test

import (
	"os/exec"
	"strings"
	"testing"
)

// The four clients dismiss a displayed receipt through the real command path:
// idle dismissal is silent and idempotent, in-flight work refuses and keeps
// the card visible, and the command vocabulary stays closed.
func TestReceiptHideCommand(t *testing.T) {
	output, err := exec.Command(luaRuntime(t), "receipt_hide.lua", "../../addon").CombinedOutput()
	if err != nil || !strings.Contains(string(output), "receipt hide: four profiles passed") {
		t.Fatalf("%v\n%s", err, output)
	}
}
