// Package addon_test drives the workbench Lua suites under tests/addon across
// the four supported client profiles with a standalone Lua 5.1 interpreter.
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

// TestTOCSharedSections keeps the four TOC files identical apart from the
// client profile line and its per-client event-catalog data line, and proves
// every referenced load file exists.
func TestTOCSharedSections(t *testing.T) {
	tocDir := filepath.Join("..", "..", "addon")
	tocs := []string{
		"Lychee Dev_Mainline.toc",
		"Lychee Dev_Mists.toc",
		"Lychee Dev_Wrath.toc",
		"Lychee Dev_Forever.toc",
	}
	var shared []string
	for index, name := range tocs {
		data, err := os.ReadFile(filepath.Join(tocDir, name))
		if err != nil {
			t.Fatal(err)
		}
		lines := strings.Split(strings.ReplaceAll(string(data), "\r\n", "\n"), "\n")
		tailStart := -1
		for lineIndex, line := range lines {
			if strings.HasPrefix(line, "Clients\\") {
				tailStart = lineIndex + 1
				break
			}
		}
		if tailStart < 0 {
			t.Fatalf("%s: client profile line missing", name)
		}
		tailLines := make([]string, 0, len(lines)-tailStart)
		for _, line := range lines[tailStart:] {
			// The per-client generated event catalog is the one shared-slot
			// line allowed to differ, mirroring the client profile line above.
			if strings.HasPrefix(line, "Modules\\Events\\CatalogData_") {
				line = "Modules\\Events\\CatalogData_<client>.lua"
			}
			tailLines = append(tailLines, line)
		}
		if index == 0 {
			shared = tailLines
			continue
		}
		if strings.Join(tailLines, "\n") != strings.Join(shared, "\n") {
			t.Fatalf("%s: shared load list differs from %s", name, tocs[0])
		}
	}
	for _, name := range tocs {
		data, err := os.ReadFile(filepath.Join(tocDir, name))
		if err != nil {
			t.Fatal(err)
		}
		for _, line := range strings.Split(string(data), "\n") {
			line = strings.TrimSpace(strings.ReplaceAll(line, "\r", ""))
			if line == "" || strings.HasPrefix(line, "#") {
				continue
			}
			relative := strings.ReplaceAll(line, "\\", string(filepath.Separator))
			if _, err := os.Stat(filepath.Join(tocDir, relative)); err != nil {
				t.Fatalf("%s: referenced file missing: %s", name, line)
			}
		}
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
