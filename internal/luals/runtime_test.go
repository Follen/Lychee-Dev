package luals

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"testing"
	"time"
	"unicode/utf8"
)

func TestReportRequiresValidCompletedJSON(t *testing.T) {
	allowed := map[string]string{strings.ToLower(filepath.Clean(`C:\source\a.lua`)): "a.lua"}
	if _, err := decodeReport([]byte(`{}`), allowed); err != nil {
		t.Fatal(err)
	}
	if _, err := decodeReport([]byte(`[]`), allowed); err != nil {
		t.Fatal(err)
	}
	for _, raw := range []string{"", `null`, `[{}]`, `{"file:///C:/else.lua":[]}`} {
		if _, err := decodeReport([]byte(raw), allowed); err == nil {
			t.Fatalf("accepted invalid report %q", raw)
		}
	}
}

func TestLSPFramesBounded(t *testing.T) {
	if _, err := readFrame(bufioReader("Content-Length: 2\r\n\r\n{}")); err != nil {
		t.Fatal(err)
	}
	if _, err := readFrame(bufioReader("Content-Length: 8388609\r\n\r\n")); !errors.Is(err, ErrBudget) {
		t.Fatalf("large frame: %v", err)
	}
}

func TestLSPResultBudgetsAndRanges(t *testing.T) {
	hover, cut, err := decodeHover(json.RawMessage(`{"contents":"` + strings.Repeat("a", 16383) + `中尾"}`))
	if err != nil || !cut || !utf8.ValidString(hover) || len(hover) > 16384 {
		t.Fatalf("hover truncation: %d %v %v", len(hover), cut, err)
	}
	workspace := filepath.Join(`C:\`, `source`)
	library := filepath.Join(`C:\`, `library`)
	raw := json.RawMessage(`[{"uri":"file:///C:/source/a.lua","range":{"start":{"line":2,"character":8},"end":{"line":2,"character":3}}}]`)
	if _, _, err := decodeLocations(raw, workspace, library); err == nil {
		t.Fatal("accepted reversed LSP range")
	}
}

func bufioReader(s string) *bufio.Reader { return bufio.NewReader(strings.NewReader(s)) }

// LYCHEEDEV_LUALS_TEST_RUNTIME points to an extracted, verified LuaLS 3.19.1
// release during Windows integration testing. This exercises the real process.
func TestRealRuntimePublicInterfaces(t *testing.T) {
	dir := os.Getenv("LYCHEEDEV_LUALS_TEST_RUNTIME")
	if dir == "" {
		t.Skip("set LYCHEEDEV_LUALS_TEST_RUNTIME for real-process test")
	}
	r := &Runtime{directory: dir, Identity: Pinned()}
	ctx, cancel := context.WithTimeout(context.Background(), 70*time.Second)
	defer cancel()
	clean, err := r.Check(ctx, map[string][]byte{"a.lua": []byte("local x = 1\n")}, nil)
	if err != nil {
		t.Fatalf("public check empty diagnostics: %v", err)
	}
	if clean.State != "checked" || !clean.Passed || len(clean.Diagnostics) != 0 {
		t.Fatalf("clean report: %+v", clean)
	}
	bad, err := r.Check(ctx, map[string][]byte{"a.lua": []byte("local x = \n")}, nil)
	if err != nil {
		t.Fatalf("public check diagnostics: %v", err)
	}
	if bad.State != "checked" || bad.Passed || len(bad.Diagnostics) == 0 {
		t.Fatalf("bad report: %+v", bad)
	}
	workspace := t.TempDir()
	if err := os.WriteFile(filepath.Join(workspace, "a.lua"), []byte("local M = {}\nfunction M.answer() return 42 end\nreturn M\n"), 0600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(workspace, "b.lua"), []byte("local M = require('a')\nlocal x = M.answer()\n"), 0600); err != nil {
		t.Fatal(err)
	}
	analysis, err := r.AnalyzeWorkspace(ctx, workspace, nil, []Query{
		// First request must see b.lua even though the client has not opened it.
		{Kind: References, Path: "a.lua", Position: Position{Line: 1, Character: 12}},
		{Kind: Definition, Path: "b.lua", Position: Position{Line: 1, Character: 13}},
		{Kind: Hover, Path: "b.lua", Position: Position{Line: 1, Character: 13}},
	})
	if err != nil {
		t.Fatalf("LSP analysis: %v", err)
	}
	if analysis.State != "complete" || len(analysis.Results) != 3 {
		t.Fatalf("analysis state: %+v", analysis)
	}
	if !analysis.Capabilities.Definition || !analysis.Capabilities.References || !analysis.Capabilities.Hover {
		t.Fatalf("capabilities: %+v", analysis.Capabilities)
	}
	if len(analysis.Results[0].Locations) < 2 {
		t.Fatalf("references before opening b.lua: %+v", analysis.Results[0])
	}
	if len(analysis.Results[1].Locations) == 0 || analysis.Results[1].Locations[0].Path != "a.lua" {
		t.Fatalf("definition: %+v", analysis.Results[1])
	}
	if analysis.Results[2].Hover == "" {
		t.Fatalf("hover: %+v", analysis.Results[2])
	}
	// A repo-supplied .luarc may ask LuaLS to execute a plugin and load an
	// outside library. Our fixed --configpath overrides both security keys.
	outside := t.TempDir()
	marker := filepath.Join(outside, "plugin-ran")
	plugin := filepath.Join(outside, "plugin.lua")
	if err := os.WriteFile(plugin, []byte("local f=io.open("+strconv.Quote(filepath.ToSlash(marker))+",'w'); f:write('bad'); f:close()\n"), 0600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(outside, "external.lua"), []byte("External = {}; function External.secret() end\n"), 0600); err != nil {
		t.Fatal(err)
	}
	rc, _ := json.Marshal(map[string]any{"Lua.runtime.plugin": plugin, "Lua.workspace.library": []string{outside}, "Lua.workspace.ignoreDir": []string{"b.lua"}, "files.exclude": map[string]bool{"**/b.lua": true}})
	if err := os.WriteFile(filepath.Join(workspace, ".luarc.json"), rc, 0600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(workspace, "c.lua"), []byte("External.secret()\n"), 0600); err != nil {
		t.Fatal(err)
	}
	isolation, err := r.AnalyzeWorkspace(ctx, workspace, nil, []Query{{Kind: References, Path: "a.lua", Position: Position{Line: 1, Character: 12}}, {Kind: Definition, Path: "c.lua", Position: Position{Line: 0, Character: 10}}})
	if err != nil {
		t.Fatalf("isolation: %v", err)
	}
	if len(isolation.Results) != 2 || len(isolation.Results[0].Locations) < 2 {
		t.Fatalf("tracked b.lua excluded by repo config: %+v", isolation.Results)
	}
	if len(isolation.Results[1].Locations) != 0 {
		t.Fatalf("outside library leaked: %+v", isolation.Results)
	}
	if _, err := os.Stat(marker); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("repository plugin executed: %v", err)
	}
	cancelled, cancelNow := context.WithCancel(context.Background())
	cancelNow()
	_, err = r.AnalyzeWorkspace(cancelled, workspace, nil, []Query{{Kind: Definition, Path: "b.lua", Position: Position{Line: 1, Character: 13}}})
	if !errors.Is(err, context.Canceled) {
		t.Fatalf("cancel: %v", err)
	}
}

func TestRealRuntimeOversizedLuaKeepsOrdinaryCoverage(t *testing.T) {
	dir := os.Getenv("LYCHEEDEV_LUALS_TEST_RUNTIME")
	if dir == "" {
		t.Skip("set LYCHEEDEV_LUALS_TEST_RUNTIME")
	}
	r := &Runtime{directory: dir, Identity: Pinned()}
	workspace := t.TempDir()
	for name, data := range map[string][]byte{
		"a.lua":         []byte("function Sample() return 1 end\n"),
		"b.lua":         []byte("local n = Sample()\n"),
		"generated.lua": []byte(strings.Repeat(" ", MaxFileBytes+1)),
	} {
		if err := os.WriteFile(filepath.Join(workspace, name), data, 0600); err != nil {
			t.Fatal(err)
		}
	}
	ctx, cancel := context.WithTimeout(context.Background(), 50*time.Second)
	defer cancel()
	result, err := r.AnalyzeWorkspace(ctx, workspace, nil, []Query{
		{Kind: References, Path: "a.lua", Position: Position{Line: 0, Character: 10}},
		{Kind: Definition, Path: "generated.lua", Position: Position{Line: 0, Character: 0}},
	})
	if err != nil {
		t.Fatal(err)
	}
	if result.State != "partial" || result.Coverage.ExcludedFiles != 1 || result.Coverage.LuaFiles != 3 || len(result.Coverage.Reasons) == 0 {
		t.Fatalf("oversized exclusion not visible: %+v", result)
	}
	if len(result.Results) != 2 || result.Results[0].State != "complete" || len(result.Results[0].Locations) < 2 || result.Results[1].State != "incomplete" {
		t.Fatalf("ordinary file lost or large file falsely completed: %+v", result.Results)
	}
}
