// Package addon_test drives the workbench Lua suites under tests/addon across
// the four client profiles represented by the test harness with a standalone
// Lua 5.1 interpreter. Forever is exercised as an unverified, non-acceptance profile.
// New suites are discovered by glob: drop a t_*.lua file into this directory.
package addon_test

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"sort"
	"strings"
	"testing"
)

func luaRuntime(t *testing.T) string {
	t.Helper()
	names := []string{"lua5.1", "lua51", "lua"}
	explicit := os.Getenv("LYCHEEDEV_LUA51")
	if explicit != "" {
		names = []string{explicit}
	}
	var failures []string
	for _, name := range names {
		path, err := exec.LookPath(name)
		if err != nil {
			failures = append(failures, err.Error())
			continue
		}
		output, err := exec.Command(path, "-v").CombinedOutput()
		if err == nil && regexp.MustCompile(`^Lua 5\.1\.[0-9]+(?:\s|$)`).Match(output) {
			return path
		}
		failures = append(failures, fmt.Sprintf("%s: %v: %s", path, err, output))
	}
	if explicit != "" || os.Getenv("LYCHEEDEV_REQUIRE_LUA51") == "1" {
		t.Fatalf("required Lua 5.1 unavailable: %v", failures)
	}
	t.Skipf("optional local Lua 5.1 unavailable: %v", failures)
	return ""
}

// TestWorkbenchLuaSuites runs every t_*.lua suite under every client profile.
func TestWorkbenchLuaSuites(t *testing.T) {
	lua := luaRuntime(t)
	clients := []string{"retail", "classic", "titan", "forever"}
	testFiles, err := filepath.Glob("t_*.lua")
	if err != nil {
		t.Fatal(err)
	}
	sort.Strings(testFiles)
	if len(testFiles) == 0 {
		t.Fatal("no t_*.lua suites discovered")
	}
	addonRoot := filepath.Join("..", "..", "addon")
	// Read every suite (and the shared harness) up front so their contents are
	// part of the Go test-cache input set; Lua child processes read them too.
	for _, helper := range []string{"run.lua", "env.lua"} {
		if _, err := os.ReadFile(helper); err != nil {
			t.Fatal(err)
		}
	}
	for _, file := range testFiles {
		if _, err := os.ReadFile(file); err != nil {
			t.Fatal(err)
		}
	}
	for _, client := range clients {
		for _, file := range testFiles {
			t.Run(file+"/"+client, func(t *testing.T) {
				output, err := exec.Command(lua, "run.lua", file, client, addonRoot).CombinedOutput()
				if err != nil {
					t.Fatalf("%v\n%s", err, output)
				}
			})
		}
	}
}

// TestUnifiedTOCContract locks the single-manifest contract: one TOC for all
// supported clients, multi-interface declaration, ClientGate leading the
// loads, all four event catalogs present, and every referenced file existing.
func TestUnifiedTOCContract(t *testing.T) {
	tocDir := filepath.Join("..", "..", "addon")
	name := "Lychee Dev.toc"
	data, err := os.ReadFile(filepath.Join(tocDir, name))
	if err != nil {
		t.Fatal(err)
	}
	lines := strings.Split(strings.ReplaceAll(string(data), "\r\n", "\n"), "\n")
	var loads []string
	for _, line := range lines {
		line = strings.TrimRight(line, "")
		if line == "" || strings.HasPrefix(line, "##") || strings.HasPrefix(line, "#") {
			continue
		}
		loads = append(loads, line)
	}
	if len(loads) == 0 {
		t.Fatal("empty TOC")
	}
	if !strings.EqualFold(loads[0], "Core\\ClientGate.lua") {
		t.Fatalf("first load must be Core\\ClientGate.lua, got %q", loads[0])
	}
	interfaceLine := ""
	for _, line := range lines {
		if strings.HasPrefix(line, "## Interface:") {
			interfaceLine = strings.TrimSpace(strings.TrimPrefix(line, "## Interface:"))
		}
	}
	declared := map[string]bool{}
	for _, part := range strings.Split(interfaceLine, ",") {
		declared[strings.TrimSpace(part)] = true
	}
	for _, iface := range []string{"120100", "50504", "38002", "16001"} {
		if !declared[iface] {
			t.Fatalf("missing declared interface %s", iface)
		}
	}
	catalogs := 0
	for _, line := range loads {
		relative := strings.ReplaceAll(line, "\\", string(filepath.Separator))
		if _, err := os.Stat(filepath.Join(tocDir, relative)); err != nil {
			t.Fatalf("referenced file missing: %s", line)
		}
		if strings.HasPrefix(line, "Modules\\Events\\CatalogData_") {
			catalogs++
		}
	}
	if catalogs != 4 {
		t.Fatalf("expected all four event catalogs, found %d", catalogs)
	}
}

// TestLocaleFilesAnchorMarker keeps the locale insertion anchor available for
// later page agents.
func TestLocaleFilesAnchorMarker(t *testing.T) {
	for _, name := range []string{"Core/Locale.lua", "Core/Locale_enUS.lua"} {
		data, err := os.ReadFile(filepath.Join("..", "..", "addon", filepath.FromSlash(name)))
		if err != nil {
			t.Fatal(err)
		}
		if !strings.Contains(string(data), "-- LOCALE:END") {
			t.Fatalf("%s: missing the -- LOCALE:END marker", name)
		}
	}

}
