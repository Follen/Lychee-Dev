package selection

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/vault"
)

func TestForeverPinnedIdentityAndCurrentAdmission(t *testing.T) {
	ctx := context.Background()
	root := t.TempDir()
	if _, err := vault.Initialize(ctx, root); err != nil {
		t.Fatal(err)
	}
	for _, product := range []string{"forever", "forever-asia-beta", "forever-cn-beta"} {
		t.Run(product, func(t *testing.T) {
			data := validDataIdentityPin()
			data.Product, data.FullBuild = product, "1.60.1.70658"
			old, err := vault.WriteMetadata(ctx, root, func(_ *vault.Store, m *vault.Metadata) (PinnedSet, error) {
				return OpenPinner(m).PinSelection(ctx, SelectionSpec{Data: &data})
			})
			if err != nil {
				t.Fatal(err)
			}
			raw, _ := json.Marshal(old)
			if strings.Contains(string(raw), "productCode") {
				t.Fatal("legacy serialization changed")
			}
			read, err := InspectSelection(ctx, root, old.ID)
			if err != nil || read.ID != old.ID {
				t.Fatalf("legacy read: %+v %v", read, err)
			}
			if _, _, err := DataIdentity(*read.Data); err != nil {
				t.Fatal(err)
			}
			for _, spec := range []SelectionSpec{{Data: &data}, {Parent: old.ID}} {
				body, _ := json.Marshal(spec)
				file := filepath.Join(t.TempDir(), "target.json")
				if err := os.WriteFile(file, body, 0600); err != nil {
					t.Fatal(err)
				}
				if _, err := ResolveSelectionFile(ctx, root, file); err == nil {
					t.Fatal("retired identity admitted through file or parent")
				}
			}
		})
	}
	formal := validDataIdentityPin()
	formal.Product, formal.ProductCode, formal.FullBuild = "forever", "wowf", "1.60.1.70658"
	if product, _, err := DataIdentity(formal); err != nil || product != "wowf" {
		t.Fatalf("%q %v", product, err)
	}
	if slot, err := PinnedDataProductSlot(formal); err != nil || slot != "wowf" {
		t.Fatalf("%q %v", slot, err)
	}
	if err := ValidateNewSelection(SelectionSpec{Data: &formal}); err != nil {
		t.Fatal(err)
	}
	legacy := formal
	legacy.ProductCode = ""
	if slot, err := PinnedDataProductSlot(legacy); err != nil || slot != "wow_classic_beta" {
		t.Fatalf("legacy route: %q %v", slot, err)
	}
	formal.ProductCode = "wow_classic_beta"
	if _, _, err := DataIdentity(formal); err == nil {
		t.Fatal("conflicting formal identity accepted")
	}
}

func TestCurrentForeverFileAndInheritedBuildAdmission(t *testing.T) {
	ctx := context.Background()
	root := t.TempDir()
	if _, err := vault.Initialize(ctx, root); err != nil {
		t.Fatal(err)
	}
	data := validDataIdentityPin()
	data.Product, data.ProductCode, data.FullBuild = "forever", "wowf", "12.1.5.70391"
	legacy, err := vault.WriteMetadata(ctx, root, func(_ *vault.Store, m *vault.Metadata) (PinnedSet, error) {
		return OpenPinner(m).PinSelection(ctx, SelectionSpec{Data: &data})
	})
	if err != nil {
		t.Fatal(err)
	}
	for _, spec := range []SelectionSpec{{Data: &data}, {Parent: legacy.ID}} {
		raw, _ := json.Marshal(spec)
		file := filepath.Join(t.TempDir(), "target.json")
		if err := os.WriteFile(file, raw, 0600); err != nil {
			t.Fatal(err)
		}
		if _, err := ResolveSelectionFile(ctx, root, file); err == nil {
			t.Error("accepted mismatched formal Forever build")
		}
	}
	if got, err := InspectSelection(ctx, root, legacy.ID); err != nil || got.ID != legacy.ID {
		t.Fatalf("historical identity changed: %v", err)
	}
	change := ChangePin{Provider: "dbcache", Product: "forever", FullBuild: "12.1.5.70391", Region: "cn", Scope: "all", FetchedAt: time.Now().UTC(), SHA256: strings.Repeat("d", 64)}
	if err := ValidateNewSelection(SelectionSpec{Changes: &change}); err == nil {
		t.Error("accepted mismatched Forever Changes build")
	}
	data.FullBuild = "1.60.1.70658"
	if err := ValidateNewSelection(SelectionSpec{Data: &data}); err != nil {
		t.Fatal(err)
	}
}
