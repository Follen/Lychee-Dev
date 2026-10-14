//go:build windows && amd64

package command

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"strings"

	"github.com/follenfang/lycheedev/internal/buildinfo"
	"github.com/follenfang/lycheedev/internal/delivery"
	"github.com/follenfang/lycheedev/internal/live"
	"github.com/follenfang/lycheedev/internal/live/channel"
	"github.com/follenfang/lycheedev/internal/testkit"
	"testing"
)

func TestLiveDoctorRejectsMutatingFlagsWithoutFix(t *testing.T) {
	for _, args := range [][]string{
		{"--release", "candidate"}, {"--output", "archive"}, {"--request", "repair"},
		{"--session", "CON-11111111111111111111111111111111", "--pid", "42"},
	} {
		opts, err := parseOptions(append([]string{"live", "doctor"}, args...))
		if err != nil {
			t.Fatal(err)
		}
		_, code, err := runLiveDoctorCommand(context.Background(), opts, &Envelope{})
		if code != 2 || err == nil {
			t.Fatalf("flags reached target discovery: %v %d %v", args, code, err)
		}
	}
}

func TestDoctorUsesLiveChecksOnlyForAnExplicitTarget(t *testing.T) {
	if got := liveDoctorChecks(context.Background(), Options{}); len(got) != 0 {
		t.Fatal(got)
	}
	for _, command := range [][]string{{"doctor"}, {"live", "doctor"}} {
		opts, err := parseOptions(append(command, "--installation", "client", "--pid", "42", "--project", "workspace"))
		if err != nil || opts.pid != 42 || opts.installation != "client" || opts.project != "workspace" || opts.fix {
			t.Fatal(opts, err)
		}
	}
	opts, err := parseOptions([]string{"live", "doctor", "--fix", "--release", "candidate"})
	if err != nil || !opts.fix || opts.release != "candidate" {
		t.Fatal(opts, err)
	}
}

func TestLiveDoctorUpgradeCreatesDefaultArchiveParent(t *testing.T) {
	ctx := context.Background()
	client := testkit.Client(t, "flavor")
	oldRelease := testkit.Release(t, "")
	const oldVersion = "3.1.1"
	// Keep a complete, valid older release, including the TOC inventory hash.
	raw, err := os.ReadFile(filepath.Join(oldRelease, "release.json"))
	if err != nil {
		t.Fatal(err)
	}
	var manifest delivery.Release
	if err := json.Unmarshal(raw, &manifest); err != nil {
		t.Fatal(err)
	}
	manifest.Version = oldVersion
	for i := range manifest.Resources {
		r := &manifest.Resources[i]
		if !strings.HasSuffix(r.Path, ".toc") {
			continue
		}
		path := filepath.Join(oldRelease, "payload", filepath.FromSlash(r.Path))
		raw, err := os.ReadFile(path)
		if err != nil {
			t.Fatal(err)
		}
		raw = []byte(strings.ReplaceAll(string(raw), "## Version: "+buildinfo.Version, "## Version: "+oldVersion))
		digest := sha256.Sum256(raw)
		r.Bytes, r.SHA256 = int64(len(raw)), hex.EncodeToString(digest[:])
		if err := os.WriteFile(path, raw, 0600); err != nil {
			t.Fatal(err)
		}
	}
	raw, err = json.Marshal(manifest)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(oldRelease, "release.json"), raw, 0600); err != nil {
		t.Fatal(err)
	}
	if _, err := delivery.InstallAddon(ctx, oldRelease, client, oldVersion); err != nil {
		t.Fatal(err)
	}
	deployment, err := delivery.InspectAddonDeployment(ctx, client)
	if err != nil || deployment.Installation.State != "managed" {
		t.Fatalf("old deployment: %+v %v", deployment, err)
	}
	project, err := channel.OpenProject(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	parent := filepath.Join(project.Root, ".lycheedev", "live", "repairs")
	if _, err := os.Stat(parent); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("fixture already contains repairs parent: %v", err)
	}
	// A zero PID stops Activate before process discovery or input. The real
	// doctor repair branch must complete the disk upgrade before reaching it.
	report := channel.DoctorResult{
		Target:     &live.ClientWindow{Client: deployment.Client},
		Deployment: &channel.DoctorDeployment{State: "managed", Version: oldVersion},
	}
	_, err = fixLiveDoctor(ctx, project, Options{release: testkit.Release(t, "")}, report)
	if err == nil || err.Error() != "live.channel_explicit_target_required" {
		t.Fatalf("repair failed before safe activation boundary: %v", err)
	}
	current, err := delivery.InspectInstallation(ctx, delivery.AddonDirectory(client), "addon")
	if err != nil || current.State != "managed" || current.Receipt.Version != buildinfo.Version {
		t.Fatalf("upgrade did not publish: %+v %v", current, err)
	}
	archives, err := filepath.Glob(filepath.Join(parent, "*", "previous"))
	if err != nil || len(archives) != 1 {
		t.Fatalf("default archive: %v %v", archives, err)
	}
	previous, err := delivery.InspectInstallation(ctx, archives[0], "addon")
	if err != nil || previous.State != "managed" || previous.Receipt.Version != oldVersion {
		t.Fatalf("old installation not retained: %+v %v", previous, err)
	}
}
