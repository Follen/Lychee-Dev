package command

import (
	"os"
	"path/filepath"
	"testing"
)

func TestReadCommandsDoNotInitializeMetadata(t *testing.T) {
	root := filepath.Join(t.TempDir(), "workspace")
	if result, code := invoke(t, "init", "--home", root, "--format=json"); code != 0 || !result.OK {
		t.Fatalf("init: %+v %d", result, code)
	}
	for _, command := range [][]string{
		{"live", "status", "OP-missing"},
		{"live", "session", "SESSION-missing"},
		{"target", "show", "PIN-missing"},
		{"evidence", "show", "CAP-missing"},
		{"evidence", "verify", "CAP-missing"},
	} {
		args := append(append([]string{}, command...), "--home", root, "--format=json")
		result, code := invoke(t, args...)
		if code == 0 || result.OK || result.Result != nil || result.Error == nil || result.Error.Code != "vault.record_missing" {
			t.Fatalf("%v: %+v %d", command, result, code)
		}
		for _, dir := range []string{"state", "locks"} {
			entries, err := os.ReadDir(filepath.Join(root, dir))
			if err != nil || len(entries) != 0 {
				t.Fatalf("%v initialized %s: %v %v", command, dir, entries, err)
			}
		}
	}
}
