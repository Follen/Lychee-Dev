package delivery

import (
	"context"
	"os"
	"os/exec"
	"path/filepath"
	"testing"
)

func TestQueueStoreRejectsJunctions(t *testing.T) {
	for _, where := range []string{"addon", "bridge", "locks"} {
		t.Run(where, func(t *testing.T) {
			root := queueStoreFixture(t)
			outside := t.TempDir()
			var link string
			switch where {
			case "addon":
				link = filepath.Join(filepath.Dir(root), "Redirected Addon")
				root = link
			case "bridge":
				link = filepath.Join(root, "Bridge")
				if err := os.Remove(filepath.Join(link, "Definitions.lua")); err != nil {
					t.Fatal(err)
				}
				if err := os.Remove(link); err != nil {
					t.Fatal(err)
				}
			case "locks":
				link = filepath.Join(filepath.Dir(root), ".lycheedev-locks")
			}
			cmd := exec.Command("powershell", "-NoProfile", "-NonInteractive", "-Command", "$ErrorActionPreference = 'Stop'; New-Item -ItemType Junction -Path $env:LYCHEEDEV_QUEUE_LINK -Target $env:LYCHEEDEV_QUEUE_OUTSIDE | Out-Null")
			cmd.Env = append(os.Environ(), "LYCHEEDEV_QUEUE_LINK="+link, "LYCHEEDEV_QUEUE_OUTSIDE="+outside)
			if output, err := cmd.CombinedOutput(); err != nil {
				t.Fatalf("junction: %v %s", err, output)
			}
			defer func() {
				if err := os.Remove(link); err != nil {
					t.Error(err)
				}
			}()
			if _, err := ChangeProbeQueue(context.Background(), root, queueFixture("OP-refuse"), false); err == nil {
				t.Fatal("junction accepted")
			}
			entries, err := os.ReadDir(outside)
			if err != nil || len(entries) != 0 {
				t.Fatalf("outside modified: %v %v", entries, err)
			}
		})
	}
}
