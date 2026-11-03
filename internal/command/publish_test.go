package command

import (
	"archive/zip"
	"bytes"
	"context"
	"encoding/json"
	"image"
	"image/png"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestCurseForgeCLIPlanAndSchemaShareNestedContract(t *testing.T) {
	dir := t.TempDir()
	var archive bytes.Buffer
	z := zip.NewWriter(&archive)
	f, err := z.Create("Example/Example.toc")
	if err != nil {
		t.Fatal(err)
	}
	_, _ = f.Write([]byte("## Interface: 120105\n"))
	if err := z.Close(); err != nil {
		t.Fatal(err)
	}
	zipPath := filepath.Join(dir, "Example.zip")
	if err := os.WriteFile(zipPath, archive.Bytes(), 0600); err != nil {
		t.Fatal(err)
	}
	input := map[string]any{"project_id": 123, "file": zipPath, "changelog": "fix", "release_type": "release", "game_versions": []int{123}, "relations": map[string]any{"projects": []any{map[string]any{"slug": "dependency", "type": "optionalDependency"}}}}
	raw, _ := json.Marshal(input)
	file := filepath.Join(dir, "input.json")
	if err := os.WriteFile(file, raw, 0600); err != nil {
		t.Fatal(err)
	}
	home := filepath.Join(dir, "absent-home")
	response, exit := invoke(t, "publish", "curseforge", "plugin", "upload", "--plan", "--request", "release-1", "--file", file, "--home", home, "--format=json")
	if exit != 0 || !response.OK {
		t.Fatalf("%d %+v", exit, response)
	}
	if _, err := os.Stat(home); !os.IsNotExist(err) {
		t.Fatalf("plan initialized home: %v", err)
	}
	schema, exit := invoke(t, "schema", "publish", "curseforge", "plugin", "upload", "--format=json")
	encoded, _ := json.Marshal(schema.Result)
	if exit != 0 || !bytes.Contains(encoded, []byte("relations")) || !bytes.Contains(encoded, []byte("optionalDependency")) {
		t.Fatalf("missing nested input contract: %d %s", exit, encoded)
	}
	for _, body := range []string{`{"project_id":123,"project_id":124}`, `{"unexpected":true}`, `{"project_id":null}`} {
		if err := os.WriteFile(file, []byte(body), 0600); err != nil {
			t.Fatal(err)
		}
		bad, exit := invoke(t, "publish", "curseforge", "plugin", "upload", "--plan", "--request", "release-1", "--file", file, "--home", home, "--format=json")
		if exit != 2 || bad.OK || bad.Error == nil || bad.Error.Code != "publish.input" || bad.Error.Details == nil {
			t.Fatalf("%s: %d %+v", body, exit, bad)
		}
	}
}

func TestPublishInputSelectionAndCancellation(t *testing.T) {
	for _, args := range [][]string{{"plugin", "upload", "--plan"}, {"session", "doctor", "--plan", "--file", "missing", "--stdin"}} {
		args = append(append([]string{"publish", "curseforge"}, args...), "--format=json")
		response, exit := invoke(t, args...)
		if exit != 2 || response.OK {
			t.Fatalf("%v: %d %+v", args, exit, response)
		}
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	var out, stderr bytes.Buffer
	code := Execute(ctx, []string{"publish", "curseforge", "session", "doctor", "--plan", "--stdin", "--format=json"}, &out, &stderr)
	if code != 7 || !strings.Contains(out.String(), "cancel") || stderr.Len() != 0 {
		t.Fatalf("%d %s %s", code, out.String(), stderr.String())
	}
}

func TestPublishPlatformContractsAndOfflinePlans(t *testing.T) {
	for _, tc := range []struct {
		platform, action, schemaAction, nested string
	}{
		{"dd", "session doctor", "plugin create", "game_type"},
		{"newbee", "creator session doctor", "creator plugin create", "properties"},
		{"blackbox", "session doctor", "config create", "configuration"},
		{"modus", "session doctor", "project edit", "image_ops"},
	} {
		t.Run(tc.platform, func(t *testing.T) {
			home := filepath.Join(t.TempDir(), "absent")
			args := append([]string{"publish", tc.platform}, strings.Fields(tc.action)...)
			args = append(args, "--plan", "--home", home, "--format=json")
			response, exit := invoke(t, args...)
			if exit != 0 || !response.OK {
				t.Fatalf("offline plan: %d %+v", exit, response)
			}
			if _, err := os.Stat(home); !os.IsNotExist(err) {
				t.Fatalf("plan initialized home: %v", err)
			}
			args = append([]string{"schema", "publish", tc.platform}, strings.Fields(tc.schemaAction)...)
			response, exit = invoke(t, append(args, "--format=json")...)
			raw, _ := json.Marshal(response.Result)
			if exit != 0 || !bytes.Contains(raw, []byte(tc.nested)) {
				t.Fatalf("nested schema: %d %s", exit, raw)
			}
		})
	}
}

func TestPublishRejectsIgnoredRequestFlag(t *testing.T) {
	file := filepath.Join(t.TempDir(), "input.json")
	if err := os.WriteFile(file, []byte(`{"request":"original-json-request"}`), 0600); err != nil {
		t.Fatal(err)
	}
	home := filepath.Join(t.TempDir(), "absent-home")
	args := []string{"publish", "modus", "backup", "status", "--file", file, "--home", home, "--format=json"}
	bad, code := invoke(t, append(append([]string{}, args...), "--request", "different-ignored-request")...)
	if code != 2 || bad.OK {
		t.Fatalf("ignored conflicting request flag: %d %+v", code, bad)
	}
	good, code := invoke(t, args...)
	if code != 5 || good.OK {
		t.Fatalf("expected missing original request: %d %+v", code, good)
	}
	if _, err := os.Stat(home); !os.IsNotExist(err) {
		t.Fatalf("status created a new home: %v", err)
	}
}

func TestDDObjectUploadPlanUsesContentReceiptWithoutRequestFlag(t *testing.T) {
	dir := t.TempDir()
	file := filepath.Join(dir, "image.png")
	f, err := os.Create(file)
	if err != nil {
		t.Fatal(err)
	}
	if err = png.Encode(f, image.NewRGBA(image.Rect(0, 0, 1, 1))); err != nil {
		t.Fatal(err)
	}
	if err = f.Close(); err != nil {
		t.Fatal(err)
	}
	input := filepath.Join(dir, "input.json")
	raw, _ := json.Marshal(map[string]any{"session": "retained-session", "file": file, "kind": "image"})
	if err = os.WriteFile(input, raw, 0600); err != nil {
		t.Fatal(err)
	}
	home := filepath.Join(dir, "absent-home")
	for _, resource := range []string{"plugin", "config", "wa"} {
		args := []string{"publish", "dd", resource, "upload", "--plan", "--file", input, "--home", home, "--format=json"}
		good, code := invoke(t, args...)
		if code != 0 || !good.OK {
			t.Fatalf("%s plan without unused request: %d %+v", resource, code, good)
		}
		bad, code := invoke(t, append(append([]string{}, args...), "--request", "unused")...)
		if code != 2 || bad.OK {
			t.Fatalf("%s accepted ignored request: %d %+v", resource, code, bad)
		}
	}
	if _, err = os.Stat(home); !os.IsNotExist(err) {
		t.Fatalf("plan initialized home: %v", err)
	}
}

func TestDDBackupPreviewAndOriginalResumeCLIAdmission(t *testing.T) {
	for _, tc := range []struct {
		action, input string
		request       bool
	}{
		{"backup delete-preview", `{"session":"task-1","backup_ids":["backup-1"]}`, false},
		{"backup delete-many", `{"session":"task-1","backup_ids":["backup-1"],"confirm_delete":true}`, true},
		{"operation resume", `{"session":"task-1","request":"original-1"}`, false},
	} {
		t.Run(tc.action, func(t *testing.T) {
			dir := t.TempDir()
			file, home := filepath.Join(dir, "input.json"), filepath.Join(dir, "absent-home")
			if err := os.WriteFile(file, []byte(tc.input), 0600); err != nil {
				t.Fatal(err)
			}
			args := append([]string{"publish", "dd"}, strings.Fields(tc.action)...)
			args = append(args, "--file", file, "--plan", "--home", home, "--format=json")
			if tc.request {
				if result, code := invoke(t, args...); code != 2 || result.OK {
					t.Fatalf("missing request accepted: %d %+v", code, result)
				}
				args = append(args, "--request", "delete-1")
			}
			if result, code := invoke(t, args...); code != 0 || !result.OK {
				t.Fatalf("plan failed: %d %+v", code, result)
			}
			if _, err := os.Stat(home); !os.IsNotExist(err) {
				t.Fatalf("plan initialized home: %v", err)
			}
		})
	}
}

func TestPublishSchemaDynamicOptionsResolveToCommands(t *testing.T) {
	for _, c := range commandContracts {
		if !strings.HasPrefix(c.Path, "publish ") {
			continue
		}
		spec, ok := publishInputContract(c.Path)
		if !ok {
			t.Errorf("%s has no input contract", c.Path)
			continue
		}
		for field, route := range spec.DynamicOptions {
			if found, _, err := findCommandContract(strings.Fields(route)); err != nil || found == nil {
				t.Errorf("%s field %s advertises unavailable command %q: %v", c.Path, field, route, err)
			}
		}
	}
}
