package delivery

import (
	"context"
	"errors"
	"os"
	"os/exec"
	"path/filepath"
	"testing"
)

func TestPayloadRejectsWindowsJunction(t *testing.T) {
	root, inventory := payloadFixture(t)
	link := filepath.Join(root, "skill", "linked")
	outside := t.TempDir()
	command := exec.Command("powershell", "-NoProfile", "-NonInteractive", "-Command", "$ErrorActionPreference = 'Stop'; New-Item -ItemType Junction -Path $env:LYCHEEDEV_TEST_LINK -Target $env:LYCHEEDEV_TEST_TARGET | Out-Null")
	command.Env = append(os.Environ(), "LYCHEEDEV_TEST_LINK="+link, "LYCHEEDEV_TEST_TARGET="+outside)
	if output, err := command.CombinedOutput(); err != nil {
		t.Fatalf("create junction: %v: %s", err, output)
	}
	defer func() {
		if err := os.Remove(link); err != nil {
			t.Error(err)
		}
	}()
	if err := VerifyPayload(context.Background(), root, inventory); !errors.Is(err, ErrPayload) {
		t.Fatalf("junction accepted: %v", err)
	}
}
