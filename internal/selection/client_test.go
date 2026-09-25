package selection

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"testing"
)

func TestClientEvidencePrecedence(t *testing.T) {
	for _, test := range []struct {
		name, folder, flavor, version, want string
		active                              []ClientBuild
	}{
		{name: "retail catalog", folder: "_retail_", flavor: "wow", want: "retail", active: []ClientBuild{{"wow", "12.1.0.69875"}}},
		{name: "reused Forever slot", folder: "_classic_beta_", flavor: "wow_forever", version: "1.60.1.69893", want: "forever"},
		// The real Forever client: the flavor names the reusable slot, the
		// launcher catalog carries the build, and version.txt does not exist.
		{name: "slot flavor resolved by catalog", folder: "_classic_beta_", flavor: "wow_classic_beta", want: "forever", active: []ClientBuild{{"wow_classic_beta", "1.60.1.70009"}}},
		// A beta slot carrying a non-Forever build stays unsupported.
		{name: "slot flavor wrong series", folder: "_classic_beta_", flavor: "wow_classic_beta", want: "", active: []ClientBuild{{"wow_classic_beta", "5.5.4.69934"}}},
		{name: "version before folder", folder: "_classic_beta_", version: "1.60.1.69893", want: "forever"},
		{name: "product before folder", folder: "custom", flavor: "wow_classic_titan", version: "3.80.2.12345", want: "titan"},
		{name: "unknown product no fallback", folder: "_retail_", flavor: "wow_beta", version: "12.1.0.69875"},
		{name: "contradictory product build", folder: "_retail_", flavor: "wow", version: "1.60.1.69893"},
		{name: "MoP test slot", folder: "_classic_beta_", version: "5.5.4.69585"},
		{name: "unidentified reused slot", folder: "_classic_beta_", active: []ClientBuild{{"wow_forever", "1.60.1.69893"}}},
		{name: "ambiguous catalog", folder: "_retail_", flavor: "wow", active: []ClientBuild{{"wow", "12.1.0.1"}, {"wow", "12.1.0.2"}}},
		{name: "missing build", folder: "_retail_", flavor: "wow"},
		{name: "unsupported series", folder: "_retail_", flavor: "wow", version: "12.2.0.12345"},
	} {
		t.Run(test.name, func(t *testing.T) {
			directory := filepath.Join(t.TempDir(), test.folder)
			if err := os.Mkdir(directory, 0700); err != nil {
				t.Fatal(err)
			}
			if test.flavor != "" {
				if err := os.WriteFile(filepath.Join(directory, ".flavor.info"), []byte("Product Flavor!STRING:0\n"+test.flavor+"\n"), 0600); err != nil {
					t.Fatal(err)
				}
			}
			if test.version != "" {
				if err := os.WriteFile(filepath.Join(directory, "version.txt"), []byte(test.version), 0600); err != nil {
					t.Fatal(err)
				}
			}
			result, err := InspectClient(context.Background(), directory, test.active)
			if test.want == "" {
				if !errors.Is(err, ErrClientIdentity) {
					t.Fatalf("%+v %v", result, err)
				}
				return
			}
			if err != nil || result.Product != test.want || result.Interface == 0 || result.TOC == "" {
				t.Fatalf("%+v %v", result, err)
			}
		})
	}
}
