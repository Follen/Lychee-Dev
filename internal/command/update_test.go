package command

import (
	"path/filepath"
	"testing"
)

func TestUpdateSelectorsAreRepeatableAndIsolated(t *testing.T) {
	opts, err := parseOptions([]string{"update", "--path", "a/lycheedev", "--path", "b/lycheedev", "--installation", "game1", "--installation", "game2", "--plan", "--home", "private", "--release", "fixture"})
	if err != nil || len(opts.updatePaths) != 2 || len(opts.updateInstallations) != 2 || !opts.plan {
		t.Fatalf("%+v %v", opts, err)
	}
	for _, args := range [][]string{{"update", "--sql", "unrelated"}, {"update", "--resume"}, {"update", "--plan", "--plan"}, {"skill", "status", "--path", "a", "--path", "b"}} {
		if _, err := parseOptions(args); err == nil {
			t.Fatalf("accepted %v", args)
		}
	}
	// With explicit selectors, failure belongs to the explicit release. It must
	// never probe the desktop, read the user's skill directory or create a home.
	root := t.TempDir()
	result, code := invoke(t, "update", "--release", filepath.Join(root, "missing"), "--path", filepath.Join(root, "lycheedev"), "--home", filepath.Join(root, "home"), "--plan", "--format=json")
	if code == 0 || result.OK {
		t.Fatalf("%+v %d", result, code)
	}
}
