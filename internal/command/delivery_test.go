package command

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/delivery"
	"github.com/follenfang/lycheedev/internal/selection"
)

func TestSkillInstallCommand(t *testing.T) {
	root := t.TempDir()
	release := delivery.Release{Schema: "lycheedev.release.v1", Version: Version, Commit: strings.Repeat("a", 40), Binaries: map[string]delivery.Resource{"windows-amd64": {Path: "native/windows-amd64/lycheedev.exe", Bytes: 1, SHA256: strings.Repeat("b", 64)}}}
	for _, name := range []string{"skill/SKILL.md", "addon/Lychee Dev.toc"} {
		content := []byte("fixture " + name)
		digest := sha256.Sum256(content)
		file := filepath.Join(root, "payload", filepath.FromSlash(name))
		if err := os.MkdirAll(filepath.Dir(file), 0700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(file, content, 0600); err != nil {
			t.Fatal(err)
		}
		release.Resources = append(release.Resources, delivery.Resource{Path: name, Bytes: int64(len(content)), SHA256: hex.EncodeToString(digest[:])})
	}
	raw, err := json.Marshal(release)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(root, "release.json"), raw, 0600); err != nil {
		t.Fatal(err)
	}
	target := filepath.Join(t.TempDir(), "lycheedev")
	for range 2 {
		result, code := invoke(t, "skill", "install", "--release", root, "--path", target, "--format=json")
		if code != 0 || !result.OK {
			t.Fatalf("%+v %d", result, code)
		}
	}
	result, code := invoke(t, "skill", "status", "--path", target, "--format=json")
	if code != 0 || result.Result.(map[string]any)["state"] != "managed" {
		t.Fatalf("%+v %d", result, code)
	}
	release.Commit = strings.Repeat("c", 40)
	raw, err = json.Marshal(release)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(root, "release.json"), raw, 0600); err != nil {
		t.Fatal(err)
	}
	upgradeArchive := filepath.Join(t.TempDir(), "upgrade")
	result, code = invoke(t, "skill", "install", "--release", root, "--path", target, "--output", upgradeArchive, "--format=json")
	if code != 0 || !result.OK {
		t.Fatalf("upgrade: %+v %d", result, code)
	}
	result, code = invoke(t, "skill", "install", "--resume", "--path", target, "--output", upgradeArchive, "--format=json")
	if code != 0 || !result.OK {
		t.Fatalf("resume: %+v %d", result, code)
	}
	if err := os.WriteFile(filepath.Join(target, "SKILL.md"), []byte("local edit"), 0600); err != nil {
		t.Fatal(err)
	}
	result, code = invoke(t, "skill", "install", "--release", root, "--path", target, "--format=json")
	if code != 3 || result.Error.Code != "delivery.installation_conflict" {
		t.Fatalf("%+v %d", result, code)
	}
	archive := filepath.Join(t.TempDir(), "saved-skill")
	result, code = invoke(t, "skill", "remove", "--path", target, "--output", archive, "--format=json")
	if code != 3 || result.Error.Code != "delivery.installation_conflict" {
		t.Fatalf("remove edit: %+v %d", result, code)
	}
	original, err := os.ReadFile(filepath.Join(root, "payload", "skill", "SKILL.md"))
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(target, "SKILL.md"), original, 0600); err != nil {
		t.Fatal(err)
	}
	result, code = invoke(t, "skill", "remove", "--path", target, "--output", archive, "--format=json")
	if code != 0 || result.Result.(map[string]any)["state"] != "archived" {
		t.Fatalf("remove: %+v %d", result, code)
	}
	result, code = invoke(t, "skill", "status", "--path", archive, "--format=json")
	if code != 0 || result.Result.(map[string]any)["state"] != "managed" {
		t.Fatalf("archive: %+v %d", result, code)
	}
}

func TestInstallationArguments(t *testing.T) {
	for _, args := range [][]string{
		{"addon", "install"},
		{"addon", "install", "--release", "release", "--path", "addon"},
		{"addon", "install", "--release", "release", "--installation", "client", "--home", "home"},
		{"addon", "install", "--release", "release", "--installation", "client", "--resume"},
		{"addon", "install", "--resume", "--installation", "client"},
		{"addon", "remove", "--installation", "client"},
		{"addon", "remove", "--installation", "client", "--output", "archive", "--release", "release"},
		{"skill", "install"}, {"skill", "status"}, {"addon", "status"},
		{"version", "--release", "elsewhere"}, {"skill", "install", "--home", "home"},
		{"skill", "status", "--release", "elsewhere"},
		{"skill", "remove", "--path", "somewhere"},
		{"version", "--output", "somewhere"},
		{"version", "--resume"},
		{"addon", "status", "--installation", "client", "--path", "addon"},
		{"addon", "status", "--installation", "client", "--max-bytes", "100"},
		{"skill", "install", "--resume", "--path", "somewhere"},
		{"skill", "install", "--resume", "--release", "release", "--path", "somewhere", "--output", "archive"},
	} {
		result, code := invoke(t, append(args, "--format=json")...)
		if code != 2 || result.OK {
			t.Fatalf("%v: %+v %d", args, result, code)
		}
	}
}

func TestAddonInstallCommand(t *testing.T) {
	root := t.TempDir()
	release := delivery.Release{Schema: "lycheedev.release.v1", Version: Version, Commit: strings.Repeat("a", 40), Binaries: map[string]delivery.Resource{"windows-amd64": {Path: "native/windows-amd64/lycheedev.exe", Bytes: 1, SHA256: strings.Repeat("b", 64)}}}
	files := map[string]string{"skill/SKILL.md": "fixture", "addon/Core/Start.lua": "local name, ns = ...\n"}
	interfaces := make([]string, 0, len(selection.VerifiedClientBaselines()))
	for _, baseline := range selection.VerifiedClientBaselines() {
		interfaces = append(interfaces, fmt.Sprint(baseline.Interface))
	}
	files["addon/"+selection.MainTOC] = fmt.Sprintf("## Interface: %s\n## Version: %s\n## SavedVariables: LycheeToolkitDB\nCore/ClientGate.lua\nCore/Start.lua\n", strings.Join(interfaces, ", "), Version)
	files["addon/Core/ClientGate.lua"] = "local name, ns = ...\n"
	for name, content := range files {
		file := filepath.Join(root, "payload", filepath.FromSlash(name))
		if err := os.MkdirAll(filepath.Dir(file), 0700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(file, []byte(content), 0600); err != nil {
			t.Fatal(err)
		}
		digest := sha256.Sum256([]byte(content))
		release.Resources = append(release.Resources, delivery.Resource{Path: name, Bytes: int64(len(content)), SHA256: hex.EncodeToString(digest[:])})
	}
	raw, err := json.Marshal(release)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(root, "release.json"), raw, 0600); err != nil {
		t.Fatal(err)
	}
	client := filepath.Join(t.TempDir(), "_retail_")
	if err := os.MkdirAll(filepath.Join(client, "Interface", "AddOns"), 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(client, ".flavor.info"), []byte("wow"), 0600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(client, "version.txt"), []byte("12.1.0.69875"), 0600); err != nil {
		t.Fatal(err)
	}
	for range 2 {
		result, code := invoke(t, "addon", "install", "--release", root, "--installation", client, "--format=json")
		if code != 0 || !result.OK {
			t.Fatalf("%+v %d", result, code)
		}
	}
	result, code := invoke(t, "addon", "status", "--installation", client, "--format=json")
	if code != 0 || result.Result.(map[string]any)["installation"].(map[string]any)["state"] != "managed" {
		t.Fatalf("%+v %d", result, code)
	}
	release.Commit = strings.Repeat("c", 40)
	raw, err = json.Marshal(release)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(root, "release.json"), raw, 0600); err != nil {
		t.Fatal(err)
	}
	archive := filepath.Join(t.TempDir(), "previous-addon")
	result, code = invoke(t, "addon", "install", "--release", root, "--installation", client, "--output", archive, "--format=json")
	if code != 0 || !result.OK {
		t.Fatalf("upgrade: %+v %d", result, code)
	}
	result, code = invoke(t, "addon", "install", "--resume", "--installation", client, "--output", archive, "--format=json")
	if code != 0 || !result.OK {
		t.Fatalf("resume: %+v %d", result, code)
	}
	removed := filepath.Join(t.TempDir(), "removed-addon")
	result, code = invoke(t, "addon", "remove", "--installation", client, "--output", removed, "--format=json")
	if code != 0 || !result.OK {
		t.Fatalf("remove: %+v %d", result, code)
	}
	result, code = invoke(t, "addon", "status", "--installation", client, "--format=json")
	if code != 0 || result.Result.(map[string]any)["installation"].(map[string]any)["state"] != "absent" {
		t.Fatalf("after removal: %+v %d", result, code)
	}
}

func TestAddonStatusUsesClientIdentity(t *testing.T) {
	client := filepath.Join(t.TempDir(), "_classic_beta_")
	if err := os.Mkdir(client, 0700); err != nil {
		t.Fatal(err)
	}
	version := filepath.Join(client, "version.txt")
	if err := os.WriteFile(version, []byte("1.60.1.69893"), 0600); err != nil {
		t.Fatal(err)
	}
	result, code := invoke(t, "addon", "status", "--installation", client, "--format=json")
	if code != 0 || !result.OK {
		t.Fatalf("%+v %d", result, code)
	}
	value := result.Result.(map[string]any)
	if value["client"].(map[string]any)["product"] != "forever" || value["installation"].(map[string]any)["state"] != "absent" {
		t.Fatalf("%+v", value)
	}
	if err := os.WriteFile(version, []byte("5.5.4.69585"), 0600); err != nil {
		t.Fatal(err)
	}
	result, code = invoke(t, "addon", "status", "--installation", client, "--format=json")
	if code != 3 || result.Error.Code != "selection.client_identity" {
		t.Fatalf("%+v %d", result, code)
	}
}
