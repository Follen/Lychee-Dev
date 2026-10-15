package protocol_test

import (
	"os/exec"
	"testing"
)

func TestIdleReceiptRecoveryUsesRealOpticalLifecycle(t *testing.T) {
	output, err := exec.Command(luaRuntime(t), "receipt_recovery.lua", "../../addon").CombinedOutput()
	if err != nil {
		t.Fatalf("%v\n%s", err, output)
	}
}
