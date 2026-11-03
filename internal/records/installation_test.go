package records

import (
	"context"
	"errors"
	"github.com/follenfang/lycheedev/internal/selection"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestInstallationRootAndClientResolveSameTitan(t *testing.T) {
	game := t.TempDir()
	client := filepath.Join(game, "_classic_titan_")
	if err := os.Mkdir(client, 0700); err != nil {
		t.Fatal(err)
	}
	for file, data := range map[string]string{
		filepath.Join(client, ".flavor.info"): "wow_classic_titan",
		filepath.Join(client, "version.txt"):  "3.80.2.69874",
		filepath.Join(game, ".build.info"):    "Product!STRING:0|Version!STRING:0|Build Key!HEX:16|CDN Key!HEX:16|Active!DEC:1\nwow_classic_titan|3.80.2.69874|" + strings.Repeat("a", 32) + "|" + strings.Repeat("b", 32) + "|1\n",
	} {
		if err := os.WriteFile(file, []byte(data), 0600); err != nil {
			t.Fatal(err)
		}
	}
	child, err := InspectClientInstallation(context.Background(), client)
	if err != nil {
		t.Fatal(err)
	}
	parent, err := InspectClientInstallation(context.Background(), game)
	if err != nil || parent != child {
		t.Fatalf("root must select the same sole Titan: root=%+v client=%+v err=%v", parent, child, err)
	}
}

func TestInstallationDiscoveryDoesNotFollowForeignClient(t *testing.T) {
	game, foreign := t.TempDir(), t.TempDir()
	for name, value := range map[string]string{".flavor.info": "wow_classic_titan", "version.txt": "3.80.2.69874"} {
		if err := os.WriteFile(filepath.Join(foreign, name), []byte(value), 0600); err != nil {
			t.Fatal(err)
		}
	}
	if err := os.Symlink(foreign, filepath.Join(game, "_classic_titan_")); err != nil {
		t.Skipf("directory symlink unavailable: %v", err)
	}
	report, err := DiscoverClientInstallations(context.Background(), game)
	if err != nil || len(report.Candidates) != 0 {
		t.Fatalf("escaped root: %+v %v", report, err)
	}
}

func TestInstallationDiscoveryDoesNotInventOrOverrideClients(t *testing.T) {
	for _, scenario := range []string{"multiple", "catalog-only", "missing-flavor", "missing-version", "conflict", "empty-folder", "foreign-unsupported", "unknown-sibling"} {
		t.Run(scenario, func(t *testing.T) {
			game := t.TempDir()
			write := func(path, data string) {
				t.Helper()
				if err := os.WriteFile(path, []byte(data), 0600); err != nil {
					t.Fatal(err)
				}
			}
			catalog := "Product!STRING:0|Version!STRING:0|Build Key!HEX:16|CDN Key!HEX:16|Active!DEC:1\n"
			row := func(product, build string) string {
				return product + "|" + build + "|" + strings.Repeat("a", 32) + "|" + strings.Repeat("b", 32) + "|1\n"
			}
			catalog += row("wow_classic_titan", "3.80.2.69874") + row("wow", "12.1.0.69875")
			write(filepath.Join(game, ".build.info"), catalog)
			client := filepath.Join(game, "_classic_titan_")
			if scenario != "catalog-only" {
				if err := os.Mkdir(client, 0700); err != nil {
					t.Fatal(err)
				}
				if scenario != "missing-flavor" && scenario != "empty-folder" {
					write(filepath.Join(client, ".flavor.info"), "wow_classic_titan")
				}
				if scenario != "missing-version" && scenario != "empty-folder" {
					version := "3.80.2.69874"
					if scenario == "conflict" {
						version = "3.80.2.69873"
					}
					write(filepath.Join(client, "version.txt"), version)
				}
			}
			if scenario == "multiple" {
				other := filepath.Join(game, "_retail_")
				if err := os.Mkdir(other, 0700); err != nil {
					t.Fatal(err)
				}
				write(filepath.Join(other, ".flavor.info"), "wow")
				write(filepath.Join(other, "version.txt"), "12.1.0.69875")
				_, err := InspectClientInstallation(context.Background(), game)
				var choice *InstallationSelectionError
				if !errors.Is(err, ErrInstallationAmbiguous) || !errors.As(err, &choice) || len(choice.Discovery.Candidates) != 2 {
					t.Fatalf("ambiguity: %v", err)
				}
				_, err = SelectClientInstallation(context.Background(), client, "retail")
				if !errors.Is(err, ErrInstallationProduct) {
					t.Fatalf("explicit client retargeted: %v", err)
				}
			}
			if scenario == "foreign-unsupported" || scenario == "unknown-sibling" {
				other := filepath.Join(game, "_beta_")
				if err := os.Mkdir(other, 0700); err != nil {
					t.Fatal(err)
				}
				flavor := "wow_beta"
				if scenario == "unknown-sibling" {
					flavor = "invalid"
				}
				write(filepath.Join(other, ".flavor.info"), flavor)
			}
			got, err := SelectClientInstallation(context.Background(), game, "titan")
			switch scenario {
			case "catalog-only", "empty-folder":
				if !errors.Is(err, ErrInstallationMissing) {
					t.Fatalf("invented installed client: %+v %v", got, err)
				}
			case "conflict", "unknown-sibling":
				if !errors.Is(err, ErrInstallationConflict) {
					t.Fatalf("overrode metadata: %+v %v", got, err)
				}
			default:
				if err != nil || got.Product != "titan" || got.FullBuild != "3.80.2.69874" {
					t.Fatalf("selection: %+v %v", got, err)
				}
			}
		})
	}
}

func TestFormalForeverDoesNotBorrowRetiredInstallations(t *testing.T) {
	game := t.TempDir()
	catalog := "Product!STRING:0|Version!STRING:0|Build Key!HEX:16|CDN Key!HEX:16|Active!DEC:1\n"
	for _, row := range []struct{ folder, flavor string }{{"_cn_beta_", "wow_cn_beta"}, {"_classic_beta_", "wow_classic_beta"}, {"_forever_", "wowf"}} {
		client := filepath.Join(game, row.folder)
		if err := os.Mkdir(client, 0700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(client, ".flavor.info"), []byte(row.flavor), 0600); err != nil {
			t.Fatal(err)
		}
		catalog += row.flavor + "|1.60.1.70658|" + strings.Repeat("a", 32) + "|" + strings.Repeat("b", 32) + "|1\n"
	}
	if err := os.WriteFile(filepath.Join(game, ".build.info"), []byte(catalog), 0600); err != nil {
		t.Fatal(err)
	}
	client, err := SelectClientInstallation(context.Background(), game, "forever")
	if err != nil || client.ProductCode != "wowf" || client.Directory != filepath.Join(game, "_forever_") {
		t.Fatalf("%+v %v", client, err)
	}
	for _, folder := range []string{"_cn_beta_", "_classic_beta_"} {
		if _, err := InspectClientInstallation(context.Background(), filepath.Join(game, folder)); err == nil {
			t.Fatal("retired installation selected", folder)
		}
	}
}

func TestChinaForeverUnsupportedBuildBlocksProductSelection(t *testing.T) {
	game := t.TempDir()
	for _, row := range []struct{ folder, flavor, build string }{
		{"_cn_beta_", "wow_cn_beta", "5.5.4.69934"},
		{"_classic_beta_", "wow_classic_beta", "1.60.1.70124"},
	} {
		client := filepath.Join(game, row.folder)
		if err := os.Mkdir(client, 0700); err != nil {
			t.Fatal(err)
		}
		for name, value := range map[string]string{".flavor.info": row.flavor, "version.txt": row.build} {
			if err := os.WriteFile(filepath.Join(client, name), []byte(value), 0600); err != nil {
				t.Fatal(err)
			}
		}
	}
	if _, err := SelectClientInstallation(context.Background(), game, "forever"); !errors.Is(err, ErrInstallationMissing) {
		t.Fatalf("retired slots should not provide a current target: %v", err)
	}
}

func TestRetainedForeverClientRechecksOriginalIdentity(t *testing.T) {
	directory := filepath.Join(t.TempDir(), "_classic_beta_")
	if err := os.Mkdir(directory, 0700); err != nil {
		t.Fatal(err)
	}
	for name, value := range map[string]string{".flavor.info": "wow_classic_beta", "version.txt": "1.60.1.70338"} {
		if err := os.WriteFile(filepath.Join(directory, name), []byte(value), 0600); err != nil {
			t.Fatal(err)
		}
	}
	expected := selection.ClientInstallation{Directory: directory, Product: "forever", ProductCode: "wow_forever", FullBuild: "1.60.1.70338", Interface: 16001, TOC: selection.MainTOC, IdentitySource: "version.txt"}
	observed, err := InspectRetainedClientInstallation(context.Background(), expected)
	if err != nil || observed.ProductCode != "wow_forever" || observed.CatalogProduct != "wow_classic_beta" {
		t.Fatalf("%+v %v", observed, err)
	}
	if _, err := InspectClientInstallation(context.Background(), directory); err == nil {
		t.Fatal("retained client admitted as new target")
	}
	for _, tc := range []struct {
		flavor string
		valid  bool
	}{
		{"product=wow_classic_beta", true},
		{"WOW_CLASSIC_BETA", true},
		{"wowf wow_classic_beta", false},
		{"wow_classic_beta wow_unknown", false},
	} {
		if err := os.WriteFile(filepath.Join(directory, ".flavor.info"), []byte(tc.flavor), 0600); err != nil {
			t.Fatal(err)
		}
		_, err := InspectRetainedClientInstallation(context.Background(), expected)
		if (err == nil) != tc.valid {
			t.Errorf("flavor %q: valid=%v err=%v", tc.flavor, tc.valid, err)
		}
	}
	if err := os.WriteFile(filepath.Join(directory, ".flavor.info"), []byte("wowf"), 0600); err != nil {
		t.Fatal(err)
	}
	if _, err := InspectRetainedClientInstallation(context.Background(), expected); err == nil {
		t.Fatal("old connection retargeted to formal wowf")
	}
}
