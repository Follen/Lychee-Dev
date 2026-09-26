package delivery_test

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/delivery"
	"github.com/follenfang/lycheedev/internal/selection"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestAddonReleaseManifestContracts(t *testing.T) {
	for _, variant := range []string{"valid", "valid-queue", "bad-queue", "nonempty-queue", "valid-xml", "xml-missing", "xml-cycle", "xml-malformed", "lua-invalid", "wrong-interface", "wrong-version", "legacy-storage", "duplicate-header", "missing-load", "escape-load", "duplicate-load", "empty-load"} {
		t.Run(variant, func(t *testing.T) {
			root := t.TempDir()
			release := delivery.Release{Schema: "lycheedev.release.v1", Version: "2.0.0-dev", Commit: strings.Repeat("a", 40), Binaries: map[string]delivery.Resource{"windows-amd64": {Path: "native/windows-amd64/lycheedev.exe", Bytes: 1, SHA256: strings.Repeat("a", 64)}}}
			files := map[string]string{"skill/SKILL.md": "skill fixture", "addon/Core/Start.lua": "local name, ns = ...\n", "addon/Core/ClientGate.lua": "local name, ns = ...\n"}
			if strings.HasSuffix(variant, "-queue") {
				var definitions []bridge.ProbeDefinition
				if variant == "nonempty-queue" {
					definitions = []bridge.ProbeDefinition{{RequestID: "OP-local", Release: "2.0.0-dev", SessionNonce: strings.Repeat("a", 32), ReloadNonce: strings.Repeat("b", 32), Character: "Paladin", Realm: "Realm", GUID: "Player-1-123", Product: "retail", Build: "12.1.0.12345", Code: "return 1"}}
				}
				source, err := bridge.EncodeProbeQueue(definitions)
				if err != nil {
					t.Fatal(err)
				}
				if variant == "bad-queue" {
					source = append(source, []byte("print('not literal')")...)
				}
				files["addon/Bridge/Definitions.lua"] = string(source)
			}
			recursive := variant == "valid-xml" || strings.HasPrefix(variant, "xml-") || variant == "lua-invalid"
			if recursive {
				files["addon/UI/Root.xml"] = `<Ui><Include file="Nested.xml" /></Ui>`
				files["addon/UI/Nested.xml"] = `<Ui><Script file="../Core/Late.lua" /></Ui>`
				files["addon/Core/Late.lua"] = "local ready = true\n"
				switch variant {
				case "xml-missing":
					files["addon/UI/Nested.xml"] = `<Ui><Script file="Missing.lua" /></Ui>`
				case "xml-cycle":
					files["addon/UI/Nested.xml"] = `<Ui><Include file="Root.xml" /></Ui>`
				case "xml-malformed":
					files["addon/UI/Nested.xml"] = `<Ui><Script`
				case "lua-invalid":
					files["addon/Core/Late.lua"] = "local = invalid"
				}
			}
			interfaces := make([]string, 0, 4)
			for _, baseline := range selection.VerifiedClientBaselines() {
				interfaces = append(interfaces, fmt.Sprint(baseline.Interface))
			}
			body := fmt.Sprintf("## Interface: %s\n## Version: 2.0.0-dev\n## SavedVariables: LycheeToolkitDB\nCore\\ClientGate.lua\nCore\\Start.lua\n", strings.Join(interfaces, ", "))
			if recursive {
				body += "UI/Root.xml\n"
			}
			switch variant {
			case "wrong-interface":
				body = strings.Replace(body, "120100", "99999", 1)
			case "missing-interface":
				body = strings.Replace(body, "120100, ", "", 1)
			case "wrong-version":
				body = strings.Replace(body, "2.0.0-dev", "1.2.0", 1)
			case "legacy-storage":
				body = strings.Replace(body, "LycheeToolkitDB", "LycheeDevDB", 1)
			case "duplicate-header":
				body += "## Version: 2.0.0-dev\n"
			case "missing-load":
				body += "Missing.lua\n"
			case "escape-load":
				body += "../outside.lua\n"
			case "duplicate-load":
				body += "Core/Start.lua\n"
			case "empty-load":
				body = strings.Replace(body, "Core\\ClientGate.lua\n", "", 1)
			}
			files["addon/"+selection.MainTOC] = body
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
			manifests, err := delivery.InspectAddonRelease(context.Background(), root, release.Version)
			if strings.HasPrefix(variant, "valid") {
				if err != nil || len(manifests) != 1 {
					t.Fatalf("%+v %v", manifests, err)
				}
				if recursive && len(manifests[0].LoadedFiles) != 6 {
					t.Fatalf("incomplete closure: %+v", manifests[0])
				}
			} else if err == nil || manifests != nil {
				t.Fatalf("accepted %s: %+v %v", variant, manifests, err)
			}
		})
	}
}
