package command

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"os"
	"path/filepath"
	"testing"

	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/testkit"
	"github.com/follenfang/lycheedev/internal/vault"
)

func TestAssetExportArguments(t *testing.T) {
	for _, args := range [][]string{
		{"asset", "export"},
		{"asset", "export", "--snapshot", "unused", "--cdn", "--file-id", "11"},
		{"asset", "export", "--snapshot", "unused", "--cdn", "--installation", "unused", "--file-id", "11", "--output", "unused"},
		{"asset", "export", "--overwrite", "--overwrite"},
		{"asset", "inspect", "--overwrite"},
		{"data", "db2", "--overwrite"},
		{"asset", "export", "--overwrite=true"},
		{"asset", "export", "--encoding", "png"},
	} {
		result, code := invoke(t, append(args, "--format=json")...)
		if code != 2 || result.OK {
			t.Fatalf("%v: %+v (%d)", args, result, code)
		}
	}
}

func TestAssetExportProjectOffline(t *testing.T) {
	// LAUNCHER alone executes a native binary directly; NODE+LAUNCHER drives
	// the npm launcher (bin/lycheedev.mjs) through node.
	node, launcher := os.Getenv("LYCHEEDEV_ASSET_NODE"), os.Getenv("LYCHEEDEV_ASSET_LAUNCHER")
	if launcher == "" && node != "" {
		t.Fatal("installed asset test requires LYCHEEDEV_ASSET_LAUNCHER")
	}
	call := func(args ...string) (Envelope, int) {
		if launcher == "" {
			return invoke(t, args...)
		}
		if node == "" {
			return invokeExecutable(t, launcher, args...)
		}
		return invokeExecutable(t, node, append([]string{launcher}, args...)...)
	}
	if launcher != "" {
		t.Logf("using installed asset launcher: %s", launcher)
	}
	ctx := context.Background()
	workspace := filepath.Join(t.TempDir(), "workspace")
	if _, err := vault.Initialize(ctx, workspace); err != nil {
		t.Fatal(err)
	}
	body := bytes.Repeat([]byte{0, 1, 0xff, '\r', '\n', 128}, 50000)
	pin := testkit.CachedAsset(t, workspace, body)
	project := t.TempDir()
	if _, err := selection.InitializeProject(ctx, project, "retail"); err != nil {
		t.Fatal(err)
	}
	if _, err := selection.LockProject(ctx, workspace, project, pin.ID); err != nil {
		t.Fatal(err)
	}
	output := filepath.Join(t.TempDir(), "中文 raw file.bin")
	args := []string{"asset", "export", "--project", project, "--cdn", "--offline", "--file-id", "11", "--output", output, "--home", workspace, "--format=json"}
	result, code := call(args...)
	if code != 0 || !result.OK || len(result.Captures) != 2 || result.Context["snapshot"] != pin.ID {
		t.Fatalf("%+v (%d), fault=%+v", result, code, result.Error)
	}
	manifest := result.Result.(map[string]any)
	content := manifest["content"].(map[string]any)
	digest := sha256.Sum256(body)
	if manifest["schema"] != "lycheedev.asset-export.v1" || manifest["encoding"] != "raw" || content["sha256"] != hex.EncodeToString(digest[:]) || content["bytes"] != float64(len(body)) {
		t.Fatal(manifest)
	}
	actual, err := os.ReadFile(output)
	if err != nil || !bytes.Equal(actual, body) {
		t.Fatalf("incorrect output: %v", err)
	}
	for _, capture := range result.Captures {
		ref := capture.(map[string]any)
		verified, code := call("evidence", "verify", ref["id"].(string), "--home", workspace, "--format=json")
		if code != 0 || !verified.OK {
			t.Fatal(verified, code)
		}
	}
	conflict, code := call(args...)
	if code != 3 || conflict.OK || conflict.Error.Code != "records.export_conflict" || conflict.Error.Stage != "export" || conflict.Result != nil {
		t.Fatal(conflict, code)
	}
	if err := os.WriteFile(output, []byte("old output"), 0600); err != nil {
		t.Fatal(err)
	}
	replaced, code := call(append(args, "--overwrite")...)
	if code != 0 || !replaced.OK {
		t.Fatal(replaced, code)
	}
	actual, err = os.ReadFile(output)
	if err != nil || !bytes.Equal(actual, body) {
		t.Fatalf("incorrect replacement: %v", err)
	}
	limited := append(append([]string{}, args...), "--overwrite", "--max-bytes", "1")
	failed, code := call(limited...)
	if code == 0 || failed.OK || failed.Result != nil {
		t.Fatal(failed, code)
	}
	actual, err = os.ReadFile(output)
	if err != nil || !bytes.Equal(actual, body) {
		t.Fatalf("failed query changed output: %v", err)
	}
}
