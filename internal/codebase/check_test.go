package codebase

import (
	"context"
	"os"
	"path/filepath"
	"testing"
)

func TestCheckClosureKeepsReferenceCoverageSeparate(t *testing.T) {
	b, pin := sourceFixture(t)
	pin = testCommit(t, b, pin, map[string]string{"Interface/Blizzard_APIDocumentationGenerated/Test.lua": `return {Type="System",Name="Test",Namespace="C_Test",Functions={{Type="Function",Name="Do"}},Events={{Type="Event",Name="Tick",LiteralName="TEST_TICK"}}}`}, "checker source")
	ctx := context.Background()
	if _, err := b.IndexSource(ctx, pin); err != nil {
		t.Fatal(err)
	}
	root := t.TempDir()
	if err := os.WriteFile(filepath.Join(root, "Addon.toc"), []byte("## Interface: 120100\nmain.lua\n"), 0600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(root, "main.lua"), []byte("C_Test.Do()\nframe:RegisterEvent('TEST_TICK')\nhandlers[key]()\nLocalExtension()\n"), 0600); err != nil {
		t.Fatal(err)
	}
	result, err := OpenChecker(b.store).CheckClosure(ctx, pin, AddonInput{Root: root, Manifest: "Addon.toc"})
	if err != nil {
		t.Fatal(err)
	}
	if !result.Load.LoadValid || !result.StaticValid || result.Resolved != 2 || result.Unresolved != 3 || result.Complete || result.InterfaceStatus != "unresolved" {
		t.Fatalf("%+v", result)
	}
	if len(result.NotChecked) == 0 {
		t.Fatal("missing verification limits")
	}
	for _, reference := range result.References {
		if reference.Status == "source-present" && len(reference.Evidence) == 0 {
			t.Fatal("source presence without location")
		}
		if reference.Name == "LocalExtension" && reference.Status != "unresolved" {
			t.Fatal("addon-defined name inferred missing API")
		}
	}
}
