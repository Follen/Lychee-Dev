package testkit

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/buildinfo"
	"github.com/follenfang/lycheedev/internal/delivery"
	"github.com/follenfang/lycheedev/internal/selection"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

const Version = buildinfo.Version

func Client(t *testing.T, identity string) string {
	t.Helper()
	root := t.TempDir()
	client := filepath.Join(root, "_retail_")
	if err := os.MkdirAll(filepath.Join(client, "Interface", "AddOns"), 0700); err != nil {
		t.Fatal(err)
	}
	switch identity {
	case "flavor":
		WriteFile(t, filepath.Join(client, ".flavor.info"), "Product Flavor!STRING:0\nwow\n")
		WriteFile(t, filepath.Join(client, "version.txt"), "12.1.0.69875\n")
	case "catalog":
		WriteFile(t, filepath.Join(root, ".build.info"), "Product!STRING:0|Version!STRING:0|Build Key!HEX:0|CDN Key!HEX:0|Active!DEC:0\nwow|12.1.0.69875|"+strings.Repeat("a", 32)+"|"+strings.Repeat("b", 32)+"|1\n")
	case "unsupported":
		WriteFile(t, filepath.Join(client, ".flavor.info"), "Product Flavor!STRING:0\nwow_beta\n")
		WriteFile(t, filepath.Join(client, "version.txt"), "12.1.0.69875\n")
	}
	return client
}

func Release(t *testing.T, variant string) string {
	t.Helper()
	root := t.TempDir()
	files := map[string][]byte{
		"skill/SKILL.md":       []byte("skill fixture\n"),
		"addon/Core/Start.lua": []byte("local name, ns = ...\n"),
		"addon/Core/ClientGate.lua": []byte("local name, ns = ...\n"),
	}
	if variant == "queue" {
		files["addon/Bridge/Definitions.lua"], _ = bridge.EncodeProbeQueue(nil)
	}
	// One manifest declares every supported interface; ClientGate selects the
	// product from the running build at load time.
	interfaces := make([]string, 0, 4)
	for _, baseline := range selection.VerifiedClientBaselines() {
		interfaces = append(interfaces, fmt.Sprint(baseline.Interface))
	}
	toc := fmt.Sprintf("## Interface: %s\n## Version: %s\n## SavedVariables: LycheeToolkitDB\nCore\\ClientGate.lua\nCore\\Start.lua\n", strings.Join(interfaces, ", "), Version)
	if variant == "queue" {
		toc += "Bridge/Definitions.lua\n"
	}
	if variant == "wrong-interface" {
		toc = strings.Replace(toc, "120100", "99999", 1)
	}
	files["addon/"+selection.MainTOC] = []byte(toc)
	resources := make([]delivery.Resource, 0, len(files))
	for name, content := range files {
		WriteFile(t, filepath.Join(root, "payload", filepath.FromSlash(name)), string(content))
		digest := sha256.Sum256(content)
		resources = append(resources, delivery.Resource{Path: name, Bytes: int64(len(content)), SHA256: hex.EncodeToString(digest[:])})
	}
	WriteFile(t, filepath.Join(root, "native", "windows-amd64", "lycheedev.exe"), "binary fixture\n")
	release := delivery.Release{Schema: "lycheedev.release.v1", Version: Version, Commit: strings.Repeat("c", 40), Binaries: map[string]delivery.Resource{
		"windows-amd64": {Path: "native/windows-amd64/lycheedev.exe", Bytes: 1, SHA256: strings.Repeat("d", 64)},
	}}
	for _, resource := range resources {
		release.Resources = append(release.Resources, resource)
	}
	raw, err := json.Marshal(release)
	if err != nil {
		t.Fatal(err)
	}
	WriteFile(t, filepath.Join(root, "release.json"), string(raw))
	return root
}

func WriteFile(t *testing.T, path, content string) {
	t.Helper()
	if err := os.MkdirAll(filepath.Dir(path), 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, []byte(content), 0600); err != nil {
		t.Fatal(err)
	}
}

// CanonicalPath resolves 8.3 short paths (GitHub runner TMP is
// C:\Users\RUNNER~1\...) and symlinks so identity comparisons compare one
// spelling. It falls back to the input when the path cannot be resolved.
func CanonicalPath(t *testing.T, path string) string {
	t.Helper()
	resolved, err := filepath.EvalSymlinks(path)
	if err != nil {
		return path
	}
	return resolved
}
