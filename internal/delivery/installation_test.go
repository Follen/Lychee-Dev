package delivery

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func installedFixture(t *testing.T) (string, InstallationReceipt) {
	t.Helper()
	root, resources := payloadFixture(t)
	receipt := InstallationReceipt{Schema: "lycheedev.installation.v1", Component: "skill", Version: "2.0.0-dev", Commit: strings.Repeat("a", 40)}
	for _, resource := range resources {
		if strings.HasPrefix(resource.Path, "skill/") {
			receipt.Resources = append(receipt.Resources, resource)
		}
	}
	target := filepath.Join(root, "skill")
	raw, err := json.Marshal(receipt)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(target, installationMarker), raw, 0600); err != nil {
		t.Fatal(err)
	}
	return target, receipt
}

func TestInstallationAssessment(t *testing.T) {
	target, _ := installedFixture(t)
	assessment, err := InspectInstallation(context.Background(), target, "skill")
	if err != nil || assessment.State != "managed" {
		t.Fatalf("%+v %v", assessment, err)
	}
	if err := os.WriteFile(filepath.Join(target, "SKILL.md"), []byte("user edit"), 0600); err != nil {
		t.Fatal(err)
	}
	assessment, err = InspectInstallation(context.Background(), target, "skill")
	if err != nil || assessment.State != "modified" {
		t.Fatalf("%+v %v", assessment, err)
	}
	if _, err := InspectInstallation(context.Background(), target, "addon"); !errors.Is(err, ErrInstallation) {
		t.Fatal(err)
	}
}

func TestInstallationDoesNotAdoptExistingContent(t *testing.T) {
	root := t.TempDir()
	for _, name := range []string{"empty", "legacy"} {
		target := filepath.Join(root, name)
		if err := os.Mkdir(target, 0700); err != nil {
			t.Fatal(err)
		}
		if name == "legacy" {
			if err := os.WriteFile(filepath.Join(target, "Tasks.lua"), []byte("foreign task"), 0600); err != nil {
				t.Fatal(err)
			}
		}
		assessment, err := InspectInstallation(context.Background(), target, "addon")
		if err != nil || assessment.State != "unmanaged" {
			t.Fatalf("%+v %v", assessment, err)
		}
	}
	assessment, err := InspectInstallation(context.Background(), filepath.Join(root, "missing"), "skill")
	if err != nil || assessment.State != "absent" {
		t.Fatalf("%+v %v", assessment, err)
	}
}

func TestInstallationRejectsUnlistedAndMissingFiles(t *testing.T) {
	for _, extra := range []bool{false, true} {
		target, _ := installedFixture(t)
		var err error
		if extra {
			err = os.WriteFile(filepath.Join(target, "foreign.lua"), []byte("task"), 0600)
		} else {
			err = os.Remove(filepath.Join(target, "SKILL.md"))
		}
		if err != nil {
			t.Fatal(err)
		}
		assessment, err := InspectInstallation(context.Background(), target, "skill")
		if err != nil || assessment.State != "modified" {
			t.Fatalf("%+v %v", assessment, err)
		}
	}
}

func TestInstallationRejectsMalformedReceipt(t *testing.T) {
	for _, raw := range []string{`null`, `{}`, `{"schema":"lycheedev.installation.v1","SCHEMA":"other"}`, `{} {}`} {
		target, _ := installedFixture(t)
		if err := os.WriteFile(filepath.Join(target, installationMarker), []byte(raw), 0600); err != nil {
			t.Fatal(err)
		}
		assessment, err := InspectInstallation(context.Background(), target, "skill")
		if !errors.Is(err, ErrInstallation) || assessment.State != "" {
			t.Fatalf("%+v %v", assessment, err)
		}
	}
}
