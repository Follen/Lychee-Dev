package delivery

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func releaseFixture(t *testing.T) (string, Release) {
	t.Helper()
	root := t.TempDir()
	release := Release{Schema: "lycheedev.release.v1", Version: "2.0.0-dev", Commit: strings.Repeat("a", 40), Binaries: map[string]Resource{
		"windows-amd64": {Path: "native/windows-amd64/lycheedev.exe", Bytes: 1, SHA256: strings.Repeat("b", 64)},
	}}
	for _, name := range []string{"skill/SKILL.md", "addon/Lychee Dev_Mainline.toc", "addon/Lychee Dev_Mists.toc", "addon/Lychee Dev_Wrath.toc", "addon/Lychee Dev_Forever.toc"} {
		content := []byte("fixture " + name)
		file := filepath.Join(root, "payload", filepath.FromSlash(name))
		if err := os.MkdirAll(filepath.Dir(file), 0700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(file, content, 0600); err != nil {
			t.Fatal(err)
		}
		digest := sha256.Sum256(content)
		release.Resources = append(release.Resources, Resource{name, int64(len(content)), hex.EncodeToString(digest[:])})
	}
	writeRelease(t, root, release)
	return root, release
}

func writeRelease(t *testing.T, root string, release Release) {
	t.Helper()
	raw, err := json.Marshal(release)
	if err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(filepath.Join(root, "release.json"), raw, 0600); err != nil {
		t.Fatal(err)
	}
}

func TestInspectRelease(t *testing.T) {
	root, expected := releaseFixture(t)
	actual, err := InspectRelease(context.Background(), root, expected.Version)
	if err != nil || actual.Commit != expected.Commit || len(actual.Resources) != 5 {
		t.Fatalf("%+v %v", actual, err)
	}
	if _, err := InspectRelease(context.Background(), root, "2.0.0"); !errors.Is(err, ErrRelease) {
		t.Fatal(err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := InspectRelease(ctx, root, expected.Version); !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
}

func TestReleaseRejectsMalformedContracts(t *testing.T) {
	for _, kind := range []string{"schema", "commit", "platform", "binary-path", "missing-skill", "missing-client", "digest", "unknown", "duplicate", "case-duplicate", "escaped-duplicate", "trailing", "oversized"} {
		t.Run(kind, func(t *testing.T) {
			root, release := releaseFixture(t)
			switch kind {
			case "schema":
				release.Schema = "future"
			case "commit":
				release.Commit = "dirty"
			case "platform":
				release.Binaries["unsupported"] = release.Binaries["windows-amd64"]
			case "binary-path":
				r := release.Binaries["windows-amd64"]
				r.Path = "../exe"
				release.Binaries["windows-amd64"] = r
			case "missing-skill":
				release.Resources = release.Resources[1:]
			case "missing-client":
				release.Resources = release.Resources[:4]
			case "digest":
				release.Resources[0].SHA256 = strings.Repeat("0", 64)
			}
			raw, err := json.Marshal(release)
			if err != nil {
				t.Fatal(err)
			}
			switch kind {
			case "unknown":
				raw = append([]byte(`{"extra":true,`), raw[1:]...)
			case "duplicate":
				raw = append([]byte(`{"version":"bad",`), raw[1:]...)
			case "case-duplicate":
				raw = append([]byte(`{"VERSION":"bad",`), raw[1:]...)
			case "escaped-duplicate":
				raw = append([]byte(`{"ver\u0073ion":"bad",`), raw[1:]...)
			case "trailing":
				raw = append(raw, []byte(` {}`)...)
			case "oversized":
				raw = []byte(strings.Repeat(" ", (1<<20)+1))
			}
			if err := os.WriteFile(filepath.Join(root, "release.json"), raw, 0600); err != nil {
				t.Fatal(err)
			}
			actual, err := InspectRelease(context.Background(), root, release.Version)
			if err == nil || actual.Schema != "" {
				t.Fatalf("partial or successful result: %+v %v", actual, err)
			}
		})
	}
}
