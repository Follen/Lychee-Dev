//go:build windows && amd64

package channel

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"testing"

	"github.com/follenfang/lycheedev/internal/live"
	"github.com/follenfang/lycheedev/internal/records"
)

func TestLegacyConnectionTargetRetainsObservedSlotIdentity(t *testing.T) {
	for _, withVersion := range []bool{true, false} {
		name := "catalog"
		if withVersion {
			name = "version"
		}
		t.Run(name, func(t *testing.T) {
			p, d, meta, _ := projectFixture(t)
			ctx := context.Background()
			directory := meta.Target.Client.Directory
			if err := os.WriteFile(filepath.Join(directory, ".flavor.info"), []byte("wow_classic_beta"), 0600); err != nil {
				t.Fatal(err)
			}
			if withVersion {
				if err := os.WriteFile(filepath.Join(directory, "version.txt"), []byte("1.60.1.70124"), 0600); err != nil {
					t.Fatal(err)
				}
			} else {
				catalog := "Product!STRING:0|Version!STRING:0|Build Key!HEX:16|CDN Key!HEX:16|Active!DEC:1\n" +
					"wow_classic_beta|1.60.1.70124|aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa|bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb|1\n"
				if err := os.WriteFile(filepath.Join(filepath.Dir(directory), ".build.info"), []byte(catalog), 0600); err != nil {
					t.Fatal(err)
				}
			}
			current, err := records.InspectClientInstallation(ctx, directory)
			if err != nil {
				t.Fatal(err)
			}
			if current.CatalogProduct != "wow_classic_beta" {
				t.Fatalf("missing observed slot: %+v", current)
			}
			meta.Target.Client = current
			raw, err := json.Marshal(meta)
			if err != nil {
				t.Fatal(err)
			}
			var legacy map[string]any
			if err := json.Unmarshal(raw, &legacy); err != nil {
				t.Fatal(err)
			}
			// The released 3.1.1 record has no catalogProduct member at all.
			delete(legacy["target"].(map[string]any)["client"].(map[string]any), "catalogProduct")
			path := p.path("connections", d.State.ID+".target.json")
			if err := writeProjectJSON(ctx, path, legacy); err != nil {
				t.Fatal(err)
			}
			before, err := os.ReadFile(path)
			if err != nil {
				t.Fatal(err)
			}
			for attempt := 0; attempt < 2; attempt++ {
				loaded, err := p.metadata(d.State.ID)
				if err != nil {
					t.Fatal(err)
				}
				if !sameConnectionTarget(loaded.Target, meta.Target) {
					t.Fatalf("unchanged legacy connection rejected: saved=%+v observed=%+v", loaded.Target, meta.Target)
				}
				if loaded.Target.Client.CatalogProduct != "" || loaded.Owner != meta.Owner {
					t.Fatal("comparison changed the saved target or owner")
				}
			}
			after, err := os.ReadFile(path)
			if err != nil || string(after) != string(before) {
				t.Fatalf("read-only recovery rewrote metadata: %v", err)
			}
		})
	}
}

func TestConnectionTargetCompatibilityRejectsIdentityChanges(t *testing.T) {
	_, _, meta, _ := projectFixture(t)
	directory := meta.Target.Client.Directory
	for name, body := range map[string]string{".flavor.info": "wow_classic_beta", "version.txt": "1.60.1.70124"} {
		if err := os.WriteFile(filepath.Join(directory, name), []byte(body), 0600); err != nil {
			t.Fatal(err)
		}
	}
	current, err := records.InspectClientInstallation(context.Background(), directory)
	if err != nil {
		t.Fatal(err)
	}
	meta.Target.Client = current
	for _, test := range []struct {
		name   string
		change func(saved, observed *live.ClientWindow)
	}{
		{"regional_slot", func(_, observed *live.ClientWindow) { observed.Client.CatalogProduct = "wow_cn_beta" }},
		{"unknown_slot", func(_, observed *live.ClientWindow) { observed.Client.CatalogProduct = "wow_beta" }},
		{"different_recorded_slot", func(saved, _ *live.ClientWindow) { saved.Client.CatalogProduct = "wow_cn_beta" }},
		{"missing_current_slot", func(saved, observed *live.ClientWindow) {
			saved.Client.CatalogProduct = "wow_classic_beta"
			observed.Client.CatalogProduct = ""
		}},
		{"directory", func(_, observed *live.ClientWindow) { observed.Client.Directory += "-other" }},
		{"build", func(_, observed *live.ClientWindow) { observed.Client.FullBuild = "1.60.1.70245" }},
		{"foreign_series", func(saved, observed *live.ClientWindow) {
			saved.Client.FullBuild, observed.Client.FullBuild = "5.5.4.69934", "5.5.4.69934"
		}},
		{"product", func(_, observed *live.ClientWindow) { observed.Client.Product = "classic" }},
		{"product_code", func(_, observed *live.ClientWindow) { observed.Client.ProductCode = "wow_classic" }},
		{"interface", func(_, observed *live.ClientWindow) { observed.Client.Interface++ }},
		{"toc", func(_, observed *live.ClientWindow) { observed.Client.TOC = "Other.toc" }},
		{"identity_source", func(_, observed *live.ClientWindow) { observed.Client.IdentitySource = "folder" }},
		{"process", func(_, observed *live.ClientWindow) { observed.Window.ProcessID++ }},
		{"process_creation", func(_, observed *live.ClientWindow) { observed.Window.ProcessStartedAt++ }},
		{"window", func(_, observed *live.ClientWindow) { observed.Window.Handle++ }},
		{"executable", func(_, observed *live.ClientWindow) { observed.Window.Executable += "-other" }},
	} {
		t.Run(test.name, func(t *testing.T) {
			saved, observed := meta.Target, meta.Target
			saved.Client.CatalogProduct = ""
			test.change(&saved, &observed)
			if sameConnectionTarget(saved, observed) {
				t.Fatal("changed connection identity accepted")
			}
		})
	}
	for _, slot := range []string{"", "wow_classic_beta", "wow_cn_beta"} {
		current := meta.Target
		current.Client.CatalogProduct = slot
		if !sameConnectionTarget(current, current) {
			t.Fatalf("unchanged target rejected for slot %q", slot)
		}
	}
}
