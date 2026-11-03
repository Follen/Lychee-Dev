package addoncheck

import (
	"context"
	"os"
	"path/filepath"
	"reflect"
	"testing"
)

func TestClientConditionalLoadClosure(t *testing.T) {
	root := t.TempDir()
	files := map[string]string{
		"Addon.toc":          "[Family]/Start.lua\nMissing.lua [AllowLoadGameType vanilla]\nShared.xml\n[Game]/Game.lua\nLast.lua [AllowLoad game]\nUnknown.lua [Future flag] [AllowLoadGameType vanilla]\n",
		"Mainline/Start.lua": "return 1", "Classic/Start.lua": "return 2", "Standard/Game.lua": "return 3", "Mists/Game.lua": "return 4",
		"Shared.xml": "<Ui><Include file=\"[Family]/Nested.xml\"/></Ui>", "Mainline/Nested.xml": "<Ui/>", "Classic/Nested.xml": "<Ui/>", "Last.lua": "return 5",
	}
	for name, raw := range files {
		p := filepath.Join(root, filepath.FromSlash(name))
		if err := os.MkdirAll(filepath.Dir(p), 0700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(p, []byte(raw), 0600); err != nil {
			t.Fatal(err)
		}
	}
	for _, ctx := range []LoadContext{{Family: "Mainline", Game: "Standard"}, {Family: "Classic", Game: "Mists"}} {
		got, err := InspectLocalLoad(context.Background(), AddonInput{Root: root, Manifest: "Addon.toc", Context: ctx})
		if err != nil {
			t.Fatal(err)
		}
		names := []string{}
		for _, doc := range got.Documents {
			names = append(names, doc.Path)
		}
		want := []string{"Addon.toc", ctx.Family + "/Start.lua", "Shared.xml", ctx.Family + "/Nested.xml", ctx.Game + "/Game.lua", "Last.lua"}
		if !got.LoadValid || !got.LoadComplete || !reflect.DeepEqual(names, want) {
			t.Fatalf("context=%+v documents=%v issues=%+v", ctx, names, got.Issues)
		}
	}
	if err := os.WriteFile(filepath.Join(root, "Addon.toc"), []byte("Unknown.lua [Future flag]\nLast.lua\n"), 0600); err != nil {
		t.Fatal(err)
	}
	got, err := InspectLocalLoad(context.Background(), AddonInput{Root: root, Manifest: "Addon.toc", Context: LoadContext{Family: "Mainline", Game: "Standard"}})
	if err != nil || !got.LoadValid || got.LoadComplete || len(got.Documents) != 2 || len(got.Issues) != 1 || got.Issues[0].Code != "load_condition_unavailable" {
		t.Fatalf("unknown=%+v %v", got, err)
	}
}

func TestManifestLoadConditionsPreserveUnknownAndExcludeMissingBranches(t *testing.T) {
	for _, tc := range []struct {
		name, manifest  string
		valid, complete bool
		documents       int
	}{
		{"matching", "## AllowLoadGameType: MAINLINE\nPresent.lua [AllowLoadGameType future, mainline]\n", true, true, 2},
		{"excluded", "## AllowLoadGameType: classic\nMissing.lua\n", false, true, 1},
		{"unknown", "## AllowLoadGameType: future\nMissing.lua\n", true, false, 1},
		{"exclude-line", "Missing.lua [ExcludeLoadGameType mainline]\nPresent.lua\n", true, true, 2},
	} {
		t.Run(tc.name, func(t *testing.T) {
			root := t.TempDir()
			if err := os.WriteFile(filepath.Join(root, "Addon.toc"), []byte(tc.manifest), 0600); err != nil {
				t.Fatal(err)
			}
			if err := os.WriteFile(filepath.Join(root, "Present.lua"), []byte("return 1"), 0600); err != nil {
				t.Fatal(err)
			}
			result, err := InspectLocalLoad(context.Background(), AddonInput{Root: root, Manifest: "Addon.toc", Context: LoadContext{Family: "Mainline", Game: "Standard"}})
			if err != nil || result.LoadValid != tc.valid || result.LoadComplete != tc.complete || len(result.Documents) != tc.documents {
				t.Fatalf("%+v %v", result, err)
			}
			for _, issue := range result.Issues {
				if issue.Code == "load_file_missing" {
					t.Fatalf("excluded/unresolved branch treated as loaded: %+v", issue)
				}
			}
		})
	}
}

// Retail 18c6d677b605b312f3d813d498404631fb941977 uses Both in
// Blizzard_APIDocumentation.toc and qualified AllowLoad headers in
// Blizzard_BNet.toc. A qualifier selects whether a header applies; it is not
// part of the header's AllowLoad value.
func TestManifestBothAndQualifiedLoadHeaders(t *testing.T) {
	for _, tc := range []struct {
		name, manifest string
		context        LoadContext
		complete       bool
	}{
		{"both", "## AllowLoad: Both\nPresent.lua\n", LoadContext{Family: "Mainline", Game: "Standard"}, true},
		{"mainline", "## AllowLoad: both [AllowLoadGameType mainline]\n## AllowLoad: glue [AllowLoadGameType classic]\nPresent.lua\n", LoadContext{Family: "Mainline", Game: "Standard"}, true},
		{"classic", "## AllowLoad: glue [AllowLoadGameType mainline]\n## AllowLoad: game [AllowLoadGameType classic]\nPresent.lua\n", LoadContext{Family: "Classic", Game: "Mists"}, true},
		{"unknown", "## AllowLoad: both [AllowLoadGameType future]\nPresent.lua\n", LoadContext{Family: "Mainline", Game: "Standard"}, false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			root := t.TempDir()
			if err := os.WriteFile(filepath.Join(root, "Addon.toc"), []byte(tc.manifest), 0600); err != nil {
				t.Fatal(err)
			}
			if err := os.WriteFile(filepath.Join(root, "Present.lua"), []byte("return 1"), 0600); err != nil {
				t.Fatal(err)
			}
			got, err := InspectLocalLoad(context.Background(), AddonInput{Root: root, Manifest: "Addon.toc", Context: tc.context})
			wantDocuments := 1
			if tc.complete {
				wantDocuments = 2
			}
			if err != nil || !got.LoadValid || got.LoadComplete != tc.complete || len(got.Documents) != wantDocuments {
				t.Fatalf("%+v err=%v", got, err)
			}
		})
	}
}
