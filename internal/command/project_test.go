package command

import (
	"context"
	"os"
	"path/filepath"
	"testing"

	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
)

func commandProject(t *testing.T) (directory, root, pin, file string) {
	t.Helper()
	root, pin, file, _ = hotfixCommandFixture(t)
	directory = t.TempDir()
	for _, args := range [][]string{
		{"project", "init", "--path", directory, "--product", "retail"},
		{"project", "lock", "--path", directory, "--snapshot", pin, "--home", root},
	} {
		result, code := invoke(t, append(args, "--format=json")...)
		if code != 0 || !result.OK {
			t.Fatalf("%v: exit=%d fault=%+v", args, code, result.Error)
		}
	}
	return
}

func TestProjectCommandsUsePortableFixedSelection(t *testing.T) {
	directory, root, pin, file := commandProject(t)
	status, code := invoke(t, "project", "status", "--path", directory, "--format=json")
	if code != 0 || resultMap(t, status)["state"] != "locked" {
		t.Fatal(status, code)
	}
	result, code := invoke(t, "data", "hotfix", "--source", "dbcache", "--project", directory, "--dbcache", file, "--home", root, "--format=json")
	if code != 0 || !result.OK || result.Context["project"] != directory {
		t.Fatalf("exit=%d fault=%+v result=%+v", code, result.Error, result)
	}

	// Only the two shareable project files cross to another directory/home.
	copyDirectory := t.TempDir()
	for _, name := range []string{"lycheedev.json", "lycheedev.lock.json"} {
		raw, err := os.ReadFile(filepath.Join(directory, name))
		if err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(copyDirectory, name), raw, 0600); err != nil {
			t.Fatal(err)
		}
	}
	newRoot := filepath.Join(t.TempDir(), "new-workspace")
	if _, err := vault.Initialize(context.Background(), newRoot); err != nil {
		t.Fatal(err)
	}
	result, code = invoke(t, "data", "hotfix", "--source", "dbcache", "--project", copyDirectory, "--dbcache", file, "--home", newRoot, "--format=json")
	if code != 0 || !result.OK {
		t.Fatalf("portable query exit=%d fault=%+v", code, result.Error)
	}
	imported, err := selection.InspectSelection(context.Background(), newRoot, pin)
	if err != nil || imported.ID != pin {
		t.Fatal(imported, err)
	}
}

func TestProjectDiscoveryAndExplicitSnapshotPrecedence(t *testing.T) {
	directory, root, pin, file := commandProject(t)
	child := filepath.Join(directory, "src", "nested")
	if err := os.MkdirAll(child, 0700); err != nil {
		t.Fatal(err)
	}
	t.Chdir(child)
	result, code := invoke(t, "data", "hotfix", "--source", "dbcache", "--dbcache", file, "--home", root, "--format=json")
	if code != 0 || result.Context["project"] != directory {
		t.Fatalf("parent project exit=%d fault=%+v", code, result.Error)
	}
	status, code := invoke(t, "project", "status", "--format=json")
	if code != 0 || resultMap(t, status)["directory"] != directory {
		t.Fatal(status, code)
	}
	if err := os.WriteFile(filepath.Join(child, "lycheedev.json"), []byte("broken"), 0600); err != nil {
		t.Fatal(err)
	}
	result, code = invoke(t, "data", "hotfix", "--source", "dbcache", "--dbcache", file, "--home", root, "--format=json")
	if code != 4 || result.Error.Code != "selection.project_format" || result.Error.Stage != "selection" {
		t.Fatal(result, code)
	}
	result, code = invoke(t, "data", "hotfix", "--source", "dbcache", "--snapshot", pin, "--project", filepath.Join(child, "absent"), "--dbcache", file, "--home", root, "--format=json")
	if code != 0 || !result.OK || result.Context["project"] != nil {
		t.Fatalf("explicit snapshot inspected project: exit=%d fault=%+v", code, result.Error)
	}
	result, code = invoke(t, "version", "--format=json")
	if code != 0 || !result.OK {
		t.Fatal("unrelated command read project", result, code)
	}
}

func TestProjectHelpAndUnlockedDoNotOpenWorkspace(t *testing.T) {
	directory := t.TempDir()
	root := filepath.Join(t.TempDir(), "uncreated")
	for _, route := range [][]string{{"source", "query"}, {"data", "db2"}, {"data", "sql"}, {"data", "hotfix"}, {"asset", "inspect"}, {"live", "bind"}} {
		result, code := invoke(t, append(route, "--project", directory, "--home", root, "--help", "--format=json")...)
		if code != 0 || !result.OK {
			t.Fatal(route, result, code)
		}
	}
	if _, err := selection.InitializeProject(context.Background(), directory, "retail"); err != nil {
		t.Fatal(err)
	}
	result, code := invoke(t, "data", "db2", "--project", directory, "--cdn", "--table", "Map", "--home", root, "--format=json")
	if code != 3 || result.Error.Code != "selection.project_unlocked" {
		t.Fatal(result, code)
	}
	if _, err := os.Stat(root); !os.IsNotExist(err) {
		t.Fatal("workspace opened before lock", err)
	}
	for _, route := range [][]string{{"live", "run"}, {"live", "resume"}, {"target", "resolve"}, {"project", "lock"}} {
		result, code := invoke(t, append(route, "--project", directory, "--format=json")...)
		if code != 2 || result.OK {
			t.Fatal("unexpected project override", route, result, code)
		}
	}
}
