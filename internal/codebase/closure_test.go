package codebase

import (
	"bytes"
	"context"
	"errors"
	"os"
	"path/filepath"
	"testing"

	"github.com/follenfang/lycheedev/internal/vault"
)

func writeClosureFixture(t *testing.T, files map[string][]byte) string {
	t.Helper()
	root := t.TempDir()
	for name, data := range files {
		path := filepath.Join(root, filepath.FromSlash(name))
		if err := os.MkdirAll(filepath.Dir(path), 0700); err != nil {
			t.Fatalf("MkdirAll(%q): %v", name, err)
		}
		if err := os.WriteFile(path, data, 0600); err != nil {
			t.Fatalf("WriteFile(%q): %v", name, err)
		}
	}
	return root
}

func inspectClosureFixture(t *testing.T, ctx context.Context, files map[string][]byte) (LoadAssessment, *vault.Store, string, error) {
	t.Helper()
	root := writeClosureFixture(t, files)
	store, err := vault.Initialize(context.Background(), filepath.Join(t.TempDir(), "home"))
	if err != nil {
		t.Fatalf("vault.Initialize: %v", err)
	}
	assessment, inspectErr := OpenChecker(store).InspectAddon(ctx, AddonInput{
		Root:     root,
		Manifest: "Addon.toc",
	})
	return assessment, store, root, inspectErr
}

func documentPaths(documents []LoadedDocument) []string {
	paths := make([]string, 0, len(documents))
	for _, document := range documents {
		paths = append(paths, document.Path)
	}
	return paths
}

func hasIssue(issues []ValidationIssue, code string) bool {
	for _, issue := range issues {
		if issue.Code == code {
			return true
		}
	}
	return false
}

func countIssues(issues []ValidationIssue, code string) int {
	count := 0
	for _, issue := range issues {
		if issue.Code == code {
			count++
		}
	}
	return count
}

func TestInspectAddonLoadsTOCAndXMLClosureDepthFirst(t *testing.T) {
	files := map[string][]byte{
		"Addon.toc":             []byte("## Interface: 120100\nCORE\\root.lua\nui\\panel.xml\n"),
		"Core/Root.lua":         []byte("Root = true\n"),
		"UI/Panel.XML":          []byte("<Ui>\n  <Include file=\"widgets\\outer.xml\" />\n  <Script file=\"scripts\\late.lua\" />\n</Ui>\n"),
		"UI/Widgets/Outer.xml":  []byte("<Ui>\n  <Script file=\"..\\scripts\\FIRST.lua\" />\n  <Include file=\"..\\scripts\\second.XML\" />\n</Ui>\n"),
		"UI/Scripts/First.LUA":  []byte("First = true\n"),
		"UI/Scripts/Second.XML": []byte("<Ui />\n"),
		"UI/Scripts/Late.lua":   []byte("Late = true\n"),
	}

	assessment, store, _, err := inspectClosureFixture(t, context.Background(), files)
	if err != nil {
		t.Fatalf("InspectAddon: %v", err)
	}
	if !assessment.LoadValid {
		t.Fatalf("closure was not valid: %#v", assessment.Issues)
	}
	if assessment.CompatibilityComplete {
		t.Fatal("closure inspection claimed compatibility completeness")
	}
	wantPaths := []string{
		"Addon.toc",
		"Core/Root.lua",
		"UI/Panel.XML",
		"UI/Widgets/Outer.xml",
		"UI/Scripts/First.LUA",
		"UI/Scripts/Second.XML",
		"UI/Scripts/Late.lua",
	}
	if got := documentPaths(assessment.Documents); !equalStrings(got, wantPaths) {
		t.Fatalf("document order = %#v, want %#v", got, wantPaths)
	}
	wantSteps := []LoadStep{
		{From: "", Line: 0, Path: "Addon.toc", Kind: "toc"},
		{From: "Addon.toc", Line: 2, Path: "Core/Root.lua", Kind: "toc"},
		{From: "Addon.toc", Line: 3, Path: "UI/Panel.XML", Kind: "toc"},
		{From: "UI/Panel.XML", Line: 2, Path: "UI/Widgets/Outer.xml", Kind: "include"},
		{From: "UI/Widgets/Outer.xml", Line: 2, Path: "UI/Scripts/First.LUA", Kind: "script"},
		{From: "UI/Widgets/Outer.xml", Line: 3, Path: "UI/Scripts/Second.XML", Kind: "include"},
		{From: "UI/Panel.XML", Line: 3, Path: "UI/Scripts/Late.lua", Kind: "script"},
	}
	if len(assessment.Steps) != len(wantSteps) {
		t.Fatalf("steps = %#v, want %#v", assessment.Steps, wantSteps)
	}
	for i := range wantSteps {
		if assessment.Steps[i] != wantSteps[i] {
			t.Fatalf("step %d = %#v, want %#v", i, assessment.Steps[i], wantSteps[i])
		}
	}
	for _, document := range assessment.Documents {
		if err := store.VerifyBlob(context.Background(), document.Blob); err != nil {
			t.Fatalf("VerifyBlob(%q): %v", document.Path, err)
		}
		want := files[document.Path]
		data, err := store.ReadBlob(context.Background(), document.Blob, maxSourceBytes)
		if err != nil {
			t.Fatalf("ReadBlob(%q): %v", document.Path, err)
		}
		if !bytes.Equal(data, want) {
			t.Fatalf("blob %q changed from original bytes: got %q, want %q", document.Path, data, want)
		}
	}
}

func TestInspectAddonReportsDuplicateAndCycle(t *testing.T) {
	files := map[string][]byte{
		"Addon.toc":  []byte("First.xml\nFirst.xml\n"),
		"First.xml":  []byte("<Ui>\n  <Include file=\"Second.xml\" />\n</Ui>\n"),
		"Second.xml": []byte("<Ui>\n  <Include file=\"First.xml\" />\n</Ui>\n"),
	}
	assessment, _, _, err := inspectClosureFixture(t, context.Background(), files)
	if err != nil {
		t.Fatalf("InspectAddon: %v", err)
	}
	if assessment.LoadValid {
		t.Fatal("duplicate/cycle closure was reported valid")
	}
	if countIssues(assessment.Issues, "duplicate_load") != 1 {
		t.Fatalf("duplicate issues = %#v", assessment.Issues)
	}
	if countIssues(assessment.Issues, "load_cycle") != 1 {
		t.Fatalf("cycle issues = %#v", assessment.Issues)
	}
	if len(assessment.Steps) != 5 {
		t.Fatalf("steps = %#v, want five retained edges", assessment.Steps)
	}
	cycle := assessment.Steps[3]
	if !cycle.Repeated || !cycle.Cycle {
		t.Fatalf("cycle edge flags = %#v", cycle)
	}
	duplicate := assessment.Steps[4]
	if !duplicate.Repeated || duplicate.Cycle {
		t.Fatalf("duplicate edge flags = %#v", duplicate)
	}
}

func TestInspectAddonRejectsUnsafeAndUnsupportedReferences(t *testing.T) {
	files := map[string][]byte{
		"Addon.toc": []byte("Missing.lua\nData.txt\n../Outside.lua\n/absolute.lua\nC:\\absolute.lua\n"),
		"Data.txt":  []byte("not a loadable source\n"),
	}
	assessment, _, _, err := inspectClosureFixture(t, context.Background(), files)
	if err != nil {
		t.Fatalf("InspectAddon: %v", err)
	}
	if assessment.LoadValid {
		t.Fatal("unsafe or unsupported references were reported valid")
	}
	for _, code := range []string{"load_file_missing", "unsupported_load"} {
		if !hasIssue(assessment.Issues, code) {
			t.Fatalf("missing %s issue: %#v", code, assessment.Issues)
		}
	}
	if got := countIssues(assessment.Issues, "load_file_missing"); got != 1 {
		t.Fatalf("missing file issues = %d, want 1: %#v", got, assessment.Issues)
	}
	if got := countIssues(assessment.Issues, "load_path_escape"); got != 3 {
		t.Fatalf("path escape issues = %d, want 3: %#v", got, assessment.Issues)
	}
	if got := documentPaths(assessment.Documents); !equalStrings(got, []string{"Addon.toc"}) {
		t.Fatalf("documents = %#v, want only manifest", got)
	}
}

func TestInspectAddonRetainsSyntaxDiagnostics(t *testing.T) {
	files := map[string][]byte{
		"Addon.toc":  []byte("Broken.lua\n"),
		"Broken.lua": []byte("function unfinished(\n"),
	}
	assessment, store, _, err := inspectClosureFixture(t, context.Background(), files)
	if err != nil {
		t.Fatalf("InspectAddon: %v", err)
	}
	if assessment.LoadValid {
		t.Fatal("syntax-invalid closure was reported valid")
	}
	if !hasIssue(assessment.Issues, "syntax") {
		t.Fatalf("syntax issue missing: %#v", assessment.Issues)
	}
	if len(assessment.Documents) != 2 || len(assessment.Documents[1].Facts.Diagnostics) == 0 {
		t.Fatalf("syntax diagnostics were not retained: %#v", assessment.Documents)
	}
	data, err := store.ReadBlob(context.Background(), assessment.Documents[1].Blob, maxSourceBytes)
	if err != nil || !bytes.Equal(data, files["Broken.lua"]) {
		t.Fatalf("syntax-invalid original bytes not retained: %q, %v", data, err)
	}
}

func TestInspectAddonHonorsContextCancellation(t *testing.T) {
	files := map[string][]byte{
		"Addon.toc": []byte("Core.lua\n"),
		"Core.lua":  []byte("Core = true\n"),
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	assessment, _, _, err := inspectClosureFixture(t, ctx, files)
	if !errors.Is(err, context.Canceled) {
		t.Fatalf("InspectAddon error = %v, want context.Canceled (assessment: %#v)", err, assessment)
	}
}

func TestInspectAddonRejectsSymlinkEscape(t *testing.T) {
	root := t.TempDir()
	outside := filepath.Join(t.TempDir(), "outside.lua")
	if err := os.WriteFile(outside, []byte("Outside = true\n"), 0600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(root, "Addon.toc"), []byte("Link.lua\n"), 0600); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(outside, filepath.Join(root, "Link.lua")); err != nil {
		t.Skipf("symlinks are not supported here: %v", err)
	}

	store, err := vault.Initialize(context.Background(), filepath.Join(t.TempDir(), "home"))
	if err != nil {
		t.Fatal(err)
	}
	assessment, err := OpenChecker(store).InspectAddon(context.Background(), AddonInput{
		Root:     root,
		Manifest: "Addon.toc",
	})
	if err != nil {
		t.Fatalf("InspectAddon: %v", err)
	}
	if assessment.LoadValid || !hasIssue(assessment.Issues, "load_unreadable") {
		t.Fatalf("symlink escape was accepted: %#v", assessment)
	}
	if got := documentPaths(assessment.Documents); !equalStrings(got, []string{"Addon.toc"}) {
		t.Fatalf("escaped document was loaded: %#v", got)
	}
}

func equalStrings(got, want []string) bool {
	if len(got) != len(want) {
		return false
	}
	for i := range want {
		if got[i] != want[i] {
			return false
		}
	}
	return true
}
