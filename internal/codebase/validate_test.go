package codebase

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
)

// workspaceFixture returns an initialized workspace and a scratch mirror ready
// for fixture commits.
func workspaceFixture(t *testing.T) (string, *Browser, selection.SourcePin) {
	t.Helper()
	ctx := context.Background()
	store, err := vault.Initialize(ctx, filepath.Join(t.TempDir(), "home"))
	if err != nil {
		t.Fatal(err)
	}
	b := OpenBrowser(store)
	if _, err := gitBytes(ctx, "", 4096, "init", "--bare", b.mirror("wow-ui-source")); err != nil {
		t.Fatal(err)
	}
	seed := selection.SourcePin{Repository: "wow-ui-source", Product: "retail", RequestedRef: "refs/heads/live", ParserRevision: ParserRevision}
	return store.Root(), b, seed
}

// evidenceTree is the pinned target snapshot used as compatibility evidence.
func evidenceTree(interfaceValue string) map[string]string {
	return map[string]string{
		"Interface/AddOns/Blizzard_APIDocumentationGenerated/Docs.lua": `return {Type="System",Name="C_Good",Namespace="C_Good",Functions={{Type="Function",Name="Do"}},Events={{Type="Event",Name="OnTick",LiteralName="GOOD_TICK"}}}`,
		"Interface/AddOns/FrameXML/Templates.xml":                      "<Ui>\n  <Frame name=\"BaseTemplate\" />\n</Ui>\n",
		"Interface/AddOns/FrameXML/Game.toc":                           "## Interface: " + interfaceValue + "\n",
	}
}

func writeAddonFiles(t *testing.T, root string, files map[string]string) {
	t.Helper()
	for name, data := range files {
		target := filepath.Join(root, filepath.FromSlash(name))
		if err := os.MkdirAll(filepath.Dir(target), 0700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(target, []byte(data), 0600); err != nil {
			t.Fatal(err)
		}
	}
}

func validationFixture(t *testing.T, addonFiles map[string]string, addonRoot string) (*Browser, selection.SourcePin, string) {
	t.Helper()
	_, b, seed := workspaceFixture(t)
	pin := testCommit(t, b, seed, evidenceTree("120100"), "evidence")
	if _, err := b.IndexSource(context.Background(), pin); err != nil {
		t.Fatalf("IndexSource: %v", err)
	}
	root := filepath.Join(t.TempDir(), addonRoot)
	writeAddonFiles(t, root, addonFiles)
	return b, pin, root
}

func issueCodes(diagnostics []ValidationDiagnostic) map[string]int {
	counts := map[string]int{}
	for _, diagnostic := range diagnostics {
		counts[diagnostic.Code]++
	}
	return counts
}

func TestValidateTOCOrderedClosureWithBOMCRLFAndBackslashes(t *testing.T) {
	addon := map[string]string{
		"Addon.toc":          "\ufeff## Interface: 120100\r\n# ignored\r\nXML\\Root.xml\r\nDirect.lua\r\n",
		"XML/Root.xml":       `<x:Ui xmlns:x="urn:test"><x:sCrIpT x:FiLe="../Nested.lua"/><x:Include file="Child.xml"/></x:Ui>`,
		"XML/Child.xml":      `<Ui><Script file="Grandchild.lua"/></Ui>`,
		"XML/Grandchild.lua": "local grandchild = true\n",
		"Nested.lua":         "local nested = true\n",
		"Direct.lua":         "local direct = true\n",
		"Outside.lua":        "this is deliberately outside the closure\n",
	}
	b, pin, root := validationFixture(t, addon, "Addon With Spaces")
	result, err := b.ValidateTOC(context.Background(), pin, "PIN-TEST", AddonInput{Root: root, Manifest: "addon.TOC"}, ValidationIdentity{})
	if err != nil {
		t.Fatal(err)
	}
	if result.Interface != "120100" {
		t.Fatalf("interface = %q", result.Interface)
	}
	if !result.Valid || len(result.Diagnostics) != 0 {
		t.Fatalf("diagnostics = %+v", result.Diagnostics)
	}
	want := []string{"XML/Root.xml", "Nested.lua", "XML/Child.xml", "XML/Grandchild.lua", "Direct.lua"}
	if len(result.LoadClosure) != len(want) {
		t.Fatalf("closure = %+v", result.LoadClosure)
	}
	kinds := []string{"toc", "script", "include", "script", "toc"}
	for i, file := range result.LoadClosure {
		if file.Path != want[i] || file.LoadOrder != i {
			t.Fatalf("closure order at %d = %+v, want %q", i, file, want[i])
		}
		if len(file.LoadedBy) != 1 || file.LoadedBy[0].Kind != kinds[i] {
			t.Fatalf("loadedBy at %d = %+v, want kind %q", i, file.LoadedBy, kinds[i])
		}
	}
	if result.CheckedLua != 3 || result.CheckedXML != 2 {
		t.Fatalf("checked counts: lua=%d xml=%d", result.CheckedLua, result.CheckedXML)
	}
	for _, file := range result.LoadClosure {
		if file.Path == "Outside.lua" {
			t.Fatal("file outside the closure was loaded")
		}
	}
}

func TestValidateTOCDuplicateAndCycleAreBoundedWarnings(t *testing.T) {
	addon := map[string]string{
		"Addon.toc":  "First.xml\nShared.lua\n",
		"First.xml":  "<Ui>\n  <Script file=\"Shared.lua\" />\n  <Include file=\"Nested.xml\" />\n</Ui>\n",
		"Nested.xml": "<Ui>\n  <Script file=\"Shared.lua\" />\n</Ui>\n",
		"Shared.lua": "return true\n",
	}
	b, pin, root := validationFixture(t, addon, "dup")
	result, err := b.ValidateTOC(context.Background(), pin, "PIN-TEST", AddonInput{Root: root, Manifest: "Addon.toc"}, ValidationIdentity{})
	if err != nil {
		t.Fatal(err)
	}
	if !result.Valid {
		t.Fatalf("bounded warnings must not invalidate: %+v", result.Diagnostics)
	}
	if counts := issueCodes(result.Diagnostics); counts["duplicate_load_reference"] != 2 {
		t.Fatalf("duplicate diagnostics = %+v", result.Diagnostics)
	}
	var shared *LoadFileRef
	for i := range result.LoadClosure {
		if result.LoadClosure[i].Path == "Shared.lua" {
			shared = &result.LoadClosure[i]
		}
	}
	if shared == nil || len(shared.LoadedBy) != 3 {
		t.Fatalf("duplicate-source traceability = %+v", shared)
	}
	if shared.LoadedBy[0] != (LoadSourceRef{File: "First.xml", Line: 2, Kind: "script"}) {
		t.Fatalf("first source = %+v", shared.LoadedBy[0])
	}

	cyclic := map[string]string{
		"Addon.toc": "A.xml\n",
		"A.xml":     "<Ui><Include file=\"B.xml\" /></Ui>",
		"B.xml":     "<Ui><Include file=\"A.xml\" /></Ui>",
	}
	b2, pin2, root2 := validationFixture(t, cyclic, "cycle")
	cycle, err := b2.ValidateTOC(context.Background(), pin2, "PIN-TEST", AddonInput{Root: root2, Manifest: "Addon.toc"}, ValidationIdentity{})
	if err != nil {
		t.Fatal(err)
	}
	if !cycle.Valid {
		t.Fatalf("bounded cycle must not invalidate: %+v", cycle.Diagnostics)
	}
	counts := issueCodes(cycle.Diagnostics)
	if counts["circular_load_reference"] != 1 {
		t.Fatalf("cycle diagnostics = %+v", cycle.Diagnostics)
	}
	if len(cycle.LoadClosure) != 2 {
		t.Fatalf("cycle must stay bounded: %+v", cycle.LoadClosure)
	}
}

func TestValidateTOCReportsClosureDiagnosticsPerCode(t *testing.T) {
	addon := map[string]string{
		"Addon.toc":  "Missing.lua\n../escape.lua\nNotes.txt\nBroken.xml\nGood.lua\n",
		"Notes.txt":  "not loadable\n",
		"Broken.xml": `<Ui><Script file="Good.lua"></Ui>`,
		"Good.lua":   "return true\n",
	}
	b, pin, root := validationFixture(t, addon, "codes")
	// The escaped target exists outside the AddOn root and must never load.
	if err := os.WriteFile(filepath.Join(filepath.Dir(root), "escape.lua"), []byte("return true\n"), 0600); err != nil {
		t.Fatal(err)
	}
	result, err := b.ValidateTOC(context.Background(), pin, "PIN-TEST", AddonInput{Root: root, Manifest: "Addon.toc"}, ValidationIdentity{})
	if err != nil {
		t.Fatal(err)
	}
	if result.Valid {
		t.Fatalf("hard errors were reported valid: %+v", result.Diagnostics)
	}
	counts := issueCodes(result.Diagnostics)
	for code, want := range map[string]int{"load_file_missing": 1, "load_path_escape": 1, "unsupported_load_type": 1, "xml_parse_failed": 1} {
		if counts[code] != want {
			t.Fatalf("code %s count = %d, want %d: %+v", code, counts[code], want, result.Diagnostics)
		}
	}
	for _, diagnostic := range result.Diagnostics {
		if diagnostic.Severity == "" || diagnostic.Message == "" || diagnostic.Evidence == nil {
			t.Fatalf("unstable diagnostic = %+v", diagnostic)
		}
	}
}

func TestValidateTOCCompatibilityCodesAndUnresolved(t *testing.T) {
	addon := map[string]string{
		"Addon.toc": "## Interface: 120100\nmain.lua\nUI.xml\n",
		"main.lua":  "C_Good.Do()\nC_Missing.Do()\nframe:RegisterEvent(\"GOOD_TICK\")\nframe:RegisterEvent(\"NOPE_TICK\")\nhandlers[GetName()]()\nframe:RegisterEvent(eventName)\nLocalExtension()\n",
		"UI.xml":    "<Ui>\n  <Frame name=\"MyFrame\" inherits=\"BaseTemplate,NoSuchTemplate\" />\n</Ui>\n",
	}
	b, pin, root := validationFixture(t, addon, "compat")
	result, err := b.ValidateTOC(context.Background(), pin, "PIN-TEST", AddonInput{Root: root, Manifest: "Addon.toc"}, ValidationIdentity{})
	if err != nil {
		t.Fatal(err)
	}
	if result.Valid {
		t.Fatalf("missing references must fail validation: %+v", result.Diagnostics)
	}
	counts := issueCodes(result.Diagnostics)
	for code := range map[string]int{"api_not_found": 1, "event_not_found": 1, "xml_template_not_found": 1} {
		if counts[code] != 1 {
			t.Fatalf("code %s count = %d: %+v", code, counts[code], result.Diagnostics)
		}
	}
	if counts["toc_interface_mismatch"] != 0 {
		t.Fatalf("matched interface reported as mismatch: %+v", result.Diagnostics)
	}
	reasons := strings.Join(func() []string {
		out := []string{}
		for _, item := range result.Unresolved {
			out = append(out, item.Reason)
		}
		return out
	}(), "\n")
	for _, want := range []string{"call target is computed", "event name is computed", "may be an AddOn-defined global"} {
		if !strings.Contains(reasons, want) {
			t.Fatalf("unresolved reason %q missing (unresolved items are not distinguished from hard errors): %+v", want, result.Unresolved)
		}
	}
	for _, fact := range result.Facts {
		if fact.Name == "C_Good.Do" && !fact.Exists {
			t.Fatal("existing API reported missing")
		}
		if fact.Name == "C_Missing.Do" && fact.Exists {
			t.Fatal("missing API reported present")
		}
	}
	if result.Coverage.Checked == 0 || result.Coverage.Resolved == 0 || result.Coverage.Unresolved == 0 {
		t.Fatalf("coverage = %+v", result.Coverage)
	}
}

func TestValidateTOCMultipleInterfaces(t *testing.T) {
	for _, declared := range []string{"120100, 50504, 38002, 16001", "50504, 120100", "1201000, 50504"} {
		t.Run(declared, func(t *testing.T) {
			addon := map[string]string{"Addon.toc": "## Interface: " + declared + "\nmain.lua\n", "main.lua": "C_Good.Do()\n"}
			b, pin, root := validationFixture(t, addon, "multiple")
			result, err := b.ValidateTOC(context.Background(), pin, "PIN-TEST", AddonInput{Root: root, Manifest: "Addon.toc"}, ValidationIdentity{})
			if err != nil {
				t.Fatal(err)
			}
			want := 0
			if strings.Contains(declared, "1201000") {
				want = 1
			}
			if got := issueCodes(result.Diagnostics)["toc_interface_mismatch"]; got != want {
				t.Fatalf("declaration %q: mismatch count %d, want %d", declared, got, want)
			}
		})
	}
}

func TestValidateTOCInterfaceMismatchAndUnknownEvidence(t *testing.T) {
	addon := map[string]string{"Addon.toc": "## Interface: 99999\nmain.lua\n", "main.lua": "C_Good.Do()\n"}
	b, pin, root := validationFixture(t, addon, "mismatch")
	result, err := b.ValidateTOC(context.Background(), pin, "PIN-TEST", AddonInput{Root: root, Manifest: "Addon.toc"}, ValidationIdentity{})
	if err != nil {
		t.Fatal(err)
	}
	counts := issueCodes(result.Diagnostics)
	if counts["toc_interface_mismatch"] != 1 || result.Valid {
		t.Fatalf("interface mismatch = %+v", result.Diagnostics)
	}

	// Without any authoritative Interface evidence the state stays unresolved;
	// it is never reported as a verified match or a hard mismatch.
	_, b2, seed := workspaceFixture(t)
	tree := evidenceTree("120100")
	delete(tree, "Interface/AddOns/FrameXML/Game.toc")
	pin2 := testCommit(t, b2, seed, tree, "no interface evidence")
	if _, err := b2.IndexSource(context.Background(), pin2); err != nil {
		t.Fatal(err)
	}
	unknown, err := b2.ValidateTOC(context.Background(), pin2, "PIN-TEST", AddonInput{Root: root, Manifest: "Addon.toc"}, ValidationIdentity{})
	if err != nil {
		t.Fatal(err)
	}
	counts = issueCodes(unknown.Diagnostics)
	if counts["toc_interface_mismatch"] != 0 {
		t.Fatalf("unknown evidence invented a mismatch: %+v", unknown.Diagnostics)
	}
	found := false
	for _, item := range unknown.Unresolved {
		if strings.Contains(item.Reason, "no authoritative Interface evidence") {
			found = true
		}
	}
	if !found {
		t.Fatalf("unknown interface evidence not reported: %+v", unknown.Unresolved)
	}
}

func TestCompatibilityMissingCodes(t *testing.T) {
	for kind, want := range map[string]string{
		"api":       "api_not_found",
		"event":     "event_not_found",
		"template":  "xml_template_not_found",
		"interface": "toc_interface_mismatch",
		"mixin":     "compatibility_reference_not_found",
	} {
		if code, message := compatibilityMissing(kind, "Name"); code != want || message == "" {
			t.Fatalf("compatibilityMissing(%q) = %q %q, want %q", kind, code, message, want)
		}
	}
}

func TestValidateLuaDirectorySyntaxOnlyScan(t *testing.T) {
	root := filepath.Join(t.TempDir(), "scan")
	writeAddonFiles(t, root, map[string]string{
		"ok.lua":          "return true\n",
		"broken.lua":      "function nope(\n",
		"nested/deep.lua": "return 1\n",
		"notes.txt":       "ignored\n",
	})
	result, err := ValidateLuaDirectory(context.Background(), root)
	if err != nil {
		t.Fatal(err)
	}
	if result.CheckedLua != 3 || result.Valid {
		t.Fatalf("scan result = %+v", result)
	}
	if len(result.Diagnostics) != 1 || result.Diagnostics[0].Code != "lua_parse_failed" || result.Diagnostics[0].Path != "broken.lua" {
		t.Fatalf("diagnostics = %+v", result.Diagnostics)
	}
}

func TestValidateSourceTOCUseCase(t *testing.T) {
	addon := map[string]string{"Addon.toc": "## Interface: 120100\nmain.lua\n", "main.lua": "C_Good.Do()\n"}
	home, b, seed := workspaceFixture(t)
	pin := testCommit(t, b, seed, evidenceTree("120100"), "evidence")
	if _, err := b.IndexSource(context.Background(), pin); err != nil {
		t.Fatal(err)
	}
	root := filepath.Join(t.TempDir(), "addon")
	writeAddonFiles(t, root, addon)
	metadata, err := b.store.OpenMetadata(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	pinned, err := selection.OpenPinner(metadata).PinSelection(context.Background(), selection.SelectionSpec{Source: &pin})
	metadata.Close()
	if err != nil {
		t.Fatal(err)
	}
	reading, err := ValidateSourceTOC(context.Background(), home, pinned.ID, AddonInput{Root: root, Manifest: "Addon.toc"})
	if err != nil {
		t.Fatal(err)
	}
	if !reading.Result.Valid || reading.Result.ResolvedCommit != pin.ExactCommit || reading.Capture.ID == "" {
		t.Fatalf("use case result = %+v", reading)
	}
}
