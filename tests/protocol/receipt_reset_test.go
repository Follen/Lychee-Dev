package protocol_test

import (
	"os/exec"
	"strings"
	"testing"
)

// The four clients recover a stuck queue through the real command path:
// malformed nonces stay non-actions, an idle reset proves liveness, a busy
// entry plus stale reentry ticket are cleared for the current actor only,
// and identity works again afterwards.
func TestReceiptResetCommand(t *testing.T) {
	output, err := exec.Command(luaRuntime(t), "receipt_reset.lua", "../../addon").CombinedOutput()
	if err != nil || !strings.Contains(string(output), "receipt reset: four profiles passed") {
		t.Fatalf("%v\n%s", err, output)
	}
}
