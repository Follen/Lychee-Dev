package delivery_test

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"net/url"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/delivery"
	"github.com/follenfang/lycheedev/internal/testkit"
)

// Use the release assembler itself: synthetic TOCs do not catch disagreement
// between the repository TOC and the assembler's retired-source exclusions.
func TestCurrentPackagedAddonInstall(t *testing.T) {
	_, file, _, ok := runtime.Caller(0)
	if !ok {
		t.Fatal("test source location unavailable")
	}
	repository := filepath.Clean(filepath.Join(filepath.Dir(file), "../.."))
	modulePath := filepath.ToSlash(filepath.Join(repository, "tools/release.mjs"))
	if !strings.HasPrefix(modulePath, "/") {
		modulePath = "/" + modulePath
	}
	moduleURL := (&url.URL{Scheme: "file", Path: modulePath}).String()
	stage := t.TempDir()
	ctx, cancel := context.WithTimeout(context.Background(), time.Minute)
	defer cancel()
	const script = `
import {readFileSync,writeFileSync} from 'node:fs';
import {join} from 'node:path';
const {stagePayload,repository} = await import(process.argv[1]);
const stage=process.argv[2], resources=[];
stagePayload(stage,resources);
const {version}=JSON.parse(readFileSync(join(repository,'release/version.json'),'utf8'));
writeFileSync(join(stage,'release.json'),JSON.stringify({
  schema:'lycheedev.release.v1',version,commit:'a'.repeat(40),
  binaries:{'windows-amd64':{path:'native/windows-amd64/lycheedev.exe',bytes:1,sha256:'b'.repeat(64)}},resources
}));`
	cmd := exec.CommandContext(ctx, "node", "--input-type=module", "-e", script, moduleURL, stage)
	if output, err := cmd.CombinedOutput(); err != nil {
		t.Fatalf("stage actual release payload (Node.js required): %v\n%s", err, output)
	}
	releaseRaw, err := os.ReadFile(filepath.Join(stage, "release.json"))
	if err != nil {
		t.Fatal(err)
	}
	var release delivery.Release
	if err = json.Unmarshal(releaseRaw, &release); err != nil {
		t.Fatal(err)
	}
	manifests, err := delivery.InspectAddonRelease(ctx, stage, release.Version)
	if err != nil || len(manifests) != 1 {
		t.Fatalf("current packaged TOC/load closure: %+v %v", manifests, err)
	}
	for _, name := range []string{"Bridge/SHA256.lua", "Bridge/DuplexProtocol.lua", "Bridge/DuplexRuntime.lua", "Bridge/ProbeExecution.lua", "Core/Runtime.lua"} {
		if !slices.Contains(manifests[0].LoadedFiles, name) {
			t.Fatalf("current duplex load missing: %s", name)
		}
	}
	if _, err = os.Stat(filepath.Join(stage, "payload/addon/Bridge/Definitions.lua")); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("retired queue entered the distribution: %v", err)
	}
	client := testkit.Client(t, "flavor")
	if _, err = delivery.InstallAddon(ctx, stage, client, release.Version); err != nil {
		t.Fatalf("install actual packaged addon: %v", err)
	}
	installed := delivery.AddonDirectory(client)
	assessment, err := delivery.InspectInstallation(ctx, installed, "addon")
	if err != nil || assessment.State != "managed" {
		t.Fatalf("installed payload integrity: %+v %v", assessment, err)
	}
	for _, resource := range release.Resources {
		if !strings.HasPrefix(resource.Path, "addon/") {
			continue
		}
		content, err := os.ReadFile(filepath.Join(installed, filepath.FromSlash(strings.TrimPrefix(resource.Path, "addon/"))))
		if err != nil {
			t.Fatal(err)
		}
		digest := sha256.Sum256(content)
		if int64(len(content)) != resource.Bytes || hex.EncodeToString(digest[:]) != resource.SHA256 {
			t.Fatalf("installed bytes differ: %s", resource.Path)
		}
	}

	// Reintroduce the original defect with a matching manifest digest so only
	// the missing load (rather than payload tampering) makes installation fail.
	toc := filepath.Join(stage, "payload/addon/Lychee Dev.toc")
	content, err := os.ReadFile(toc)
	if err != nil {
		t.Fatal(err)
	}
	content = append(content, []byte("\nBridge\\Definitions.lua\n")...)
	if err = os.WriteFile(toc, content, 0600); err != nil {
		t.Fatal(err)
	}
	digest := sha256.Sum256(content)
	for i := range release.Resources {
		if release.Resources[i].Path == "addon/Lychee Dev.toc" {
			release.Resources[i].Bytes = int64(len(content))
			release.Resources[i].SHA256 = hex.EncodeToString(digest[:])
		}
	}
	releaseRaw, err = json.Marshal(release)
	if err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(filepath.Join(stage, "release.json"), releaseRaw, 0600); err != nil {
		t.Fatal(err)
	}
	if _, err = delivery.InspectAddonRelease(ctx, stage, release.Version); !errors.Is(err, delivery.ErrPayload) || !strings.Contains(err.Error(), "Definitions.lua") {
		t.Fatalf("missing retired load must remain rejected: %v", err)
	}
	blockedClient := testkit.Client(t, "flavor")
	if _, err = delivery.InstallAddon(ctx, stage, blockedClient, release.Version); !errors.Is(err, delivery.ErrPayload) {
		t.Fatalf("invalid actual payload installed: %v", err)
	}
	if _, err = os.Stat(delivery.AddonDirectory(blockedClient)); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("invalid payload changed installation: %v", err)
	}
}
