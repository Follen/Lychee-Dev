package codebase

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/selection"
)

func writeMatrixConfig(t *testing.T, directory, name, contents string) string {
	t.Helper()
	target := filepath.Join(directory, name)
	if err := os.WriteFile(target, []byte(contents), 0600); err != nil {
		t.Fatal(err)
	}
	return target
}

func TestReadMatrixConfigRejectsUnknownFields(t *testing.T) {
	dir := t.TempDir()
	file := writeMatrixConfig(t, dir, "matrix.json", `{"path":".","targets":[{"id":"a","toc":"A.toc","product":"retail","ref":"refs/heads/live"}],"extra":true}`)
	if _, _, err := ReadMatrixConfig(file); err == nil || !strings.Contains(err.Error(), "codebase.invalid_matrix_config") {
		t.Fatalf("unknown field accepted: %v", err)
	}
	file = writeMatrixConfig(t, dir, "nested.json", `{"targets":[{"id":"a","toc":"A.toc","product":"retail","ref":"refs/heads/live","mystery":1}]}`)
	if _, _, err := ReadMatrixConfig(file); err == nil || !strings.Contains(err.Error(), "codebase.invalid_matrix_config") {
		t.Fatalf("nested unknown field accepted: %v", err)
	}
}

func TestReadMatrixConfigRejectsDuplicateIDAndMissingFields(t *testing.T) {
	dir := t.TempDir()
	file := writeMatrixConfig(t, dir, "dupe.json", `{"targets":[{"id":"a","toc":"A.toc","product":"retail","ref":"refs/heads/live"},{"id":"a","toc":"B.toc","product":"retail","ref":"refs/heads/live"}]}`)
	if _, _, err := ReadMatrixConfig(file); err == nil || !strings.Contains(err.Error(), "codebase.duplicate_target_id") {
		t.Fatalf("duplicate id accepted: %v", err)
	}
	for name, contents := range map[string]string{
		"noref.json":    `{"targets":[{"id":"a","toc":"A.toc","product":"retail"}]}`,
		"emptyref.json": `{"targets":[{"id":"a","toc":"A.toc","product":"retail","ref":""}]}`,
		"notarget.json": `{"targets":[]}`,
	} {
		file := writeMatrixConfig(t, dir, name, contents)
		if _, _, err := ReadMatrixConfig(file); err == nil || !strings.Contains(err.Error(), "codebase.invalid_matrix_config") {
			t.Fatalf("%s accepted: %v", name, err)
		}
	}
}

func TestReadMatrixConfigResolvesPathRelativeToMatrixFile(t *testing.T) {
	dir := t.TempDir()
	file := writeMatrixConfig(t, dir, "matrix.json", `{"path":"sub/dir","targets":[{"id":"a","toc":"A.toc","product":"retail","ref":"refs/heads/live"}]}`)
	_, addonRoot, err := ReadMatrixConfig(file)
	if err != nil {
		t.Fatal(err)
	}
	want, _ := filepath.Abs(filepath.Join(dir, "sub", "dir"))
	if addonRoot != filepath.Clean(want) {
		t.Fatalf("addon root = %q, want %q", addonRoot, want)
	}
	file = writeMatrixConfig(t, dir, "default.json", `{"targets":[{"id":"a","toc":"A.toc","product":"retail","ref":"refs/heads/live"}]}`)
	_, addonRoot, err = ReadMatrixConfig(file)
	if err != nil {
		t.Fatal(err)
	}
	want, _ = filepath.Abs(dir)
	if addonRoot != filepath.Clean(want) {
		t.Fatalf("default addon root = %q, want %q", addonRoot, want)
	}
}

func TestMergeMatrixKeepsPerTargetDiagnosticIdentity(t *testing.T) {
	shared := ValidationDiagnostic{Severity: "warning", Code: "SHARED", File: "Core.lua", Line: 8, Message: "shared"}
	pair := ValidationDiagnostic{Severity: "error", Code: "PAIRED", File: "Feature.lua", Line: 4, Message: "paired"}
	targets := []TOCValidation{
		{
			ID: "retail", Valid: true, Interface: "120100",
			LoadClosure: []LoadFileRef{{Path: "Retail.lua"}, {Path: "Shared.xml"}, {Path: "Core.lua"}},
			Facts: []ValidationFact{
				{Kind: "api", Name: "C_Common.Shared", Exists: true, Signature: "()"},
				{Kind: "api", Name: "C_Retail.Only", Exists: true, Signature: "()"},
				{Kind: "event", Name: "SHARED_EVENT", Exists: true, Signature: "(unit)"},
				{Kind: "interface", Name: "120100", Exists: true, Signature: "120100"},
			},
			Diagnostics: []ValidationDiagnostic{shared, pair},
			Unresolved:  []UnresolvedItem{{Kind: "api", Expression: "a", File: "Retail.lua", Line: 3}},
		},
		{
			ID: "classic", Valid: false, Interface: "50504",
			LoadClosure: []LoadFileRef{{Path: "Classic.lua"}, {Path: "Core.lua"}, {Path: "Shared.xml"}},
			Facts: []ValidationFact{
				{Kind: "api", Name: "C_Common.Shared", Exists: true, Signature: "()"},
				{Kind: "event", Name: "SHARED_EVENT", Exists: true, Signature: "(unit)"},
				{Kind: "interface", Name: "50504", Exists: false, Signature: ""},
			},
			Diagnostics: []ValidationDiagnostic{shared},
			Unresolved:  []UnresolvedItem{},
		},
	}
	merged := MergeMatrix("addon", targets)
	if merged.Valid {
		t.Fatal("merged matrix should be invalid when any target is invalid")
	}
	if merged.Path != "addon" || merged.Targets[0].ID != "retail" || merged.Targets[1].ID != "classic" {
		t.Fatalf("target identity lost: %+v", merged.Targets)
	}
	if len(merged.Summary.SharedDiagnostics) != 1 || merged.Summary.SharedDiagnostics[0].Code != "SHARED" {
		t.Fatalf("shared diagnostics = %+v", merged.Summary.SharedDiagnostics)
	}
	if len(merged.Summary.TargetOnlyDiagnostics["retail"]) != 1 || merged.Summary.TargetOnlyDiagnostics["retail"][0].Code != "PAIRED" {
		t.Fatalf("retail-only diagnostics = %+v", merged.Summary.TargetOnlyDiagnostics)
	}
	if len(merged.Summary.SharedFiles) != 2 || len(merged.Summary.TargetOnlyFiles["retail"]) != 1 || len(merged.Summary.TargetOnlyFiles["classic"]) != 1 {
		t.Fatalf("files = %+v / %+v", merged.Summary.SharedFiles, merged.Summary.TargetOnlyFiles)
	}
	if names := merged.Summary.APIs.Shared; len(names) != 1 || names[0] != "C_Common.Shared" {
		t.Fatalf("shared APIs = %+v", names)
	}
	if len(merged.Summary.APIs.Differences) != 1 || merged.Summary.APIs.Differences[0].Name != "C_Retail.Only" {
		t.Fatalf("API differences = %+v", merged.Summary.APIs.Differences)
	}
	if _, ok := merged.Summary.Interfaces["retail"].(map[string]any); !ok {
		t.Fatalf("interface summary = %+v", merged.Summary.Interfaces)
	}
	raw, err := json.Marshal(merged)
	if err != nil {
		t.Fatal(err)
	}
	var shape map[string]any
	if err := json.Unmarshal(raw, &shape); err != nil {
		t.Fatal(err)
	}
	summary := shape["summary"].(map[string]any)
	for _, key := range []string{"interfaces", "sharedFiles", "targetOnlyFiles", "sharedDiagnostics", "targetOnlyDiagnostics", "unresolved"} {
		if value, ok := summary[key]; !ok || value == nil {
			t.Fatalf("summary field %q missing or null: %s", key, raw)
		}
	}
}

func TestValidateSourceMatrixResolvesFixedEvidencePerTarget(t *testing.T) {
	home, b, seed := workspaceFixture(t)
	pin := testCommit(t, b, seed, evidenceTree("120100"), "evidence")
	if _, err := b.IndexSource(context.Background(), pin); err != nil {
		t.Fatal(err)
	}
	root := filepath.Join(t.TempDir(), "addon")
	writeAddonFiles(t, root, map[string]string{
		"AddonA.toc": "## Interface: 120100\nbad.lua\n",
		"bad.lua":    "C_Missing.Do()\n",
		"AddonB.toc": "## Interface: 120100\ngood.lua\n",
		"good.lua":   "C_Good.Do()\n",
	})
	file := writeMatrixConfig(t, filepath.Dir(root), "matrix.json", `{"path":`+strconv.Quote(filepath.Base(root))+`,"targets":[
 {"id":"a","toc":"AddonA.toc","product":"retail","ref":"refs/heads/live"},
 {"id":"b","toc":"AddonB.toc","source":"wow-ui-source","product":"retail","ref":"refs/heads/live"}]}`)
	resolves := 0
	resolve := func(ctx context.Context, repository, product, ref string) (selection.SourcePin, error) {
		resolves++
		if repository != "wow-ui-source" || product != "retail" {
			t.Fatalf("resolver received %q %q", repository, product)
		}
		return pin, nil
	}
	reading, err := ValidateSourceMatrix(context.Background(), home, file, resolve)
	if err != nil {
		t.Fatal(err)
	}
	if resolves != 2 {
		t.Fatalf("resolver calls = %d", resolves)
	}
	merged := reading.Result
	if len(merged.Targets) != 2 || merged.Targets[0].ID != "a" || merged.Targets[1].ID != "b" {
		t.Fatalf("targets = %+v", merged.Targets)
	}
	for _, target := range merged.Targets {
		if target.ResolvedCommit != pin.ExactCommit || target.Ref != "refs/heads/live" {
			t.Fatalf("fixed evidence lost: %+v", target)
		}
	}
	if merged.Valid {
		t.Fatalf("matrix should be invalid: %+v", merged.Summary.TargetOnlyDiagnostics)
	}
	codes := []string{}
	for _, diagnostic := range merged.Summary.TargetOnlyDiagnostics["a"] {
		codes = append(codes, diagnostic.Code)
	}
	if len(codes) != 1 || codes[0] != "api_not_found" {
		t.Fatalf("target-only diagnostics = %+v", merged.Summary.TargetOnlyDiagnostics)
	}
	if len(merged.Summary.TargetOnlyDiagnostics["b"]) != 0 {
		t.Fatalf("target b diagnostics = %+v", merged.Summary.TargetOnlyDiagnostics["b"])
	}
	if reading.Capture.ID == "" {
		t.Fatal("matrix run produced no capture")
	}
}
