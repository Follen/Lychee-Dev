//go:build windows

package publish

import (
	"context"
	"encoding/json"
	"os"
	"os/exec"
	"path/filepath"
	"testing"
)

func TestBlackboxBackupOutputResolvesDirectoryJunction(t *testing.T) {
	for _, inside := range []bool{true, false} {
		name := "outside"
		if inside {
			name = "inside"
		}
		t.Run(name, func(t *testing.T) {
			in, root := bbBackupFixture(t)
			target := root
			if !inside {
				target = t.TempDir()
			}
			alias := filepath.Join(t.TempDir(), "destination")
			if output, err := exec.Command("cmd.exe", "/c", "mklink", "/J", alias, target).CombinedOutput(); err != nil {
				t.Fatalf("create directory junction: %v: %s", err, output)
			}
			in["output_dir"] = filepath.Join(alias, "backup")
			raw, err := json.Marshal(in)
			if err != nil {
				t.Fatal(err)
			}
			result, err := RunBlackbox(context.Background(), "backup generate", Request{Input: raw})
			if inside {
				if err == nil || result.Complete {
					t.Fatal("output junction into selected game accepted")
				}
				if _, err := os.Stat(filepath.Join(root, "backup")); !os.IsNotExist(err) {
					t.Fatal("rejected output was created inside selected game")
				}
				return
			}
			if err != nil || !result.Complete {
				t.Fatalf("outside junction backup: %+v, %v", result, err)
			}
			file := String(result.Data.(map[string]any), "file")
			if _, err := bbLoadBackupPackage(context.Background(), file); err != nil {
				t.Fatal(err)
			}
		})
	}
}
