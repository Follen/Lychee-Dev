package publish

import (
	"archive/zip"
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func publishZip(t *testing.T) string {
	t.Helper()
	path := filepath.Join(t.TempDir(), "测试 Addon.zip")
	f, err := os.Create(path)
	if err != nil {
		t.Fatal(err)
	}
	w := zip.NewWriter(f)
	entry, err := w.Create("Addon/Addon.toc")
	if err != nil {
		t.Fatal(err)
	}
	entry.Write([]byte("## Interface: 120105\n"))
	if err = w.Close(); err != nil {
		t.Fatal(err)
	}
	f.Close()
	return path
}
func cfUploadInput(t *testing.T, file string) map[string]any {
	t.Helper()
	raw, _ := json.Marshal(map[string]any{"project_id": 123, "file": file, "changelog": "fix", "release_type": "release", "game_versions": []int{42}, "is_marked_for_manual_release": false})
	spec, _ := CurseForgeInput("plugin upload")
	input, err := spec.Decode(raw)
	if err != nil {
		t.Fatal(err)
	}
	return input
}
func TestPublishInputPreservesValuesAndRejectsAmbiguity(t *testing.T) {
	spec := InputSpec{Properties: map[string]Field{"empty": {Type: "string"}, "zero": {Type: "integer", Minimum: Int(0)}, "false": {Type: "boolean"}, "null": {Type: "string", Nullable: true}, "array": {Type: "array", Items: &Field{Type: "integer"}}}}
	got, err := spec.Decode([]byte(`{"empty":"","zero":0,"false":false,"null":null,"array":[]}`))
	if err != nil || len(got) != 5 || got["false"] != false || got["null"] != nil {
		t.Fatalf("%#v %v", got, err)
	}
	for _, raw := range []string{`{"zero":0,"zero":1}`, `{"zero":0,"\u007aero":1}`, `{"zero":false}`, `{"zero":1.5}`, `{"zero":9223372036854775808}`, `{"array":[null]}`, `{"unknown":0}`, `{"empty":null}`, `{} {}`} {
		if _, err := spec.Decode([]byte(raw)); err == nil {
			t.Fatal("accepted ambiguous input", raw)
		}
	}
}
func TestCurseForgeUploadWireReceiptAndNoReplay(t *testing.T) {
	file := publishZip(t)
	archive, _ := os.ReadFile(file)
	a, err := inspectArtifact(context.Background(), file)
	if err != nil {
		t.Fatal(err)
	}
	a.file.Close()
	uploads, queries := 0, 0
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/api/projects/123/upload-file":
			uploads++
			if r.Header.Get("X-Api-Token") != "upload-secret" || r.Header.Get("X-Api-Key") != "" {
				t.Error("mixed credentials")
			}
			if err := r.ParseMultipartForm(1 << 20); err != nil {
				t.Error(err)
				http.Error(w, "bad", 400)
				return
			}
			var metadata map[string]any
			if err := json.Unmarshal([]byte(r.FormValue("metadata")), &metadata); err != nil {
				t.Error(err)
			}
			if metadata["isMarkedForManualRelease"] != false || metadata["releaseType"] != "release" {
				t.Errorf("wire %#v", metadata)
			}
			body, _, err := r.FormFile("file")
			if err != nil {
				t.Error(err)
				return
			}
			raw, _ := io.ReadAll(body)
			body.Close()
			if !bytes.Equal(raw, archive) {
				t.Error("upload bytes changed")
			}
			io.WriteString(w, `{"id":456}`)
		case "/v1/mods/123/files/456":
			queries++
			if r.Header.Get("X-Api-Key") != "query-secret" || r.Header.Get("X-Api-Token") != "" {
				t.Error("mixed credentials")
			}
			fmt.Fprintf(w, `{"data":{"id":456,"modId":123,"fileLength":%d,"hashes":[{"algo":1,"value":%q}]}}`, len(archive), a.sha1)
		default:
			t.Error("unexpected URL", r.URL.Path)
			http.NotFound(w, r)
		}
	}))
	defer server.Close()
	c := &cfClient{home: t.TempDir(), http: NewHTTP(), core: server.URL, upload: server.URL, config: map[string]string{"CURSEFORGE_API_KEY": "query-secret", "CURSEFORGE_UPLOAD_TOKEN": "upload-secret"}}
	input := cfUploadInput(t, file)
	result, err := c.uploadFile(context.Background(), input, "release-1")
	if err != nil || result.Complete || result.Submission != "acknowledged" {
		t.Fatalf("%+v %v", result, err)
	}
	if _, err = c.uploadFile(context.Background(), input, "release-1"); err != nil || uploads != 1 {
		t.Fatalf("upload replay: %d %v", uploads, err)
	}
	verified, err := c.verify(context.Background(), 123, "release-1", 456)
	if err != nil || !verified.Complete || verified.Verification != "remote_file_identity" || queries != 1 {
		t.Fatalf("%+v %v", verified, err)
	}
	input["changelog"] = "changed"
	if _, err = c.uploadFile(context.Background(), input, "release-1"); err == nil || uploads != 1 {
		t.Fatal("changed request replayed")
	}
}
func TestCurseForgeUnknownUploadStaysRecoverable(t *testing.T) {
	uploads := 0
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { uploads++; http.Error(w, "secret platform detail", 500) }))
	defer server.Close()
	c := &cfClient{home: t.TempDir(), http: NewHTTP(), upload: server.URL, config: map[string]string{"CURSEFORGE_UPLOAD_TOKEN": "secret"}}
	input := cfUploadInput(t, publishZip(t))
	result, err := c.uploadFile(context.Background(), input, "unknown-1")
	if err == nil || strings.Contains(err.Error(), "secret platform") {
		t.Fatalf("error %v", err)
	}
	if result.Submission != "intent_recorded" {
		t.Fatal(result)
	}
	if _, err = c.uploadFile(context.Background(), input, "unknown-1"); err != nil || uploads != 1 {
		t.Fatalf("unknown replay %d %v", uploads, err)
	}
}

func TestCurseForgeRejectedUploadCannotVerifyExistingFile(t *testing.T) {
	for _, status := range []int{http.StatusUnauthorized, http.StatusForbidden, http.StatusInternalServerError} {
		t.Run(fmt.Sprint(status), func(t *testing.T) {
			file := publishZip(t)
			a, err := inspectArtifact(context.Background(), file)
			if err != nil {
				t.Fatal(err)
			}
			a.file.Close()
			uploads, reads := 0, 0
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				if r.Method == http.MethodPost {
					uploads++
					io.Copy(io.Discard, r.Body)
					http.Error(w, "upload denied", status)
					return
				}
				reads++
				fmt.Fprintf(w, `{"data":{"id":456,"modId":123,"fileLength":%d,"hashes":[{"algo":1,"value":%q}]}}`, a.size, a.sha1)
			}))
			defer server.Close()
			c := &cfClient{home: t.TempDir(), http: NewHTTP(), core: server.URL, upload: server.URL, config: map[string]string{"CURSEFORGE_API_KEY": "query", "CURSEFORGE_UPLOAD_TOKEN": "upload"}}
			input := cfUploadInput(t, file)
			if _, err := c.uploadFile(context.Background(), input, "denied"); err == nil {
				t.Fatal("unsuccessful upload accepted")
			}
			result, err := c.verify(context.Background(), 123, "denied", 456)
			if status == http.StatusInternalServerError {
				if err != nil || !result.Complete || reads != 1 {
					t.Fatalf("unknown upload recovery: %+v, %v", result, err)
				}
			} else {
				if err == nil || result.Complete || result.Submission != "rejected" || reads != 0 {
					t.Fatalf("rejected upload verified from existing bytes: %+v, %v, reads=%d", result, err, reads)
				}
			}
			repeated, repeatErr := c.uploadFile(context.Background(), input, "denied")
			if uploads != 1 {
				t.Fatalf("original request replayed: uploads=%d", uploads)
			}
			if status == http.StatusInternalServerError {
				if repeatErr != nil || !repeated.Complete {
					t.Fatalf("verified original request not retained: %+v, %v", repeated, repeatErr)
				}
			} else {
				platformError, ok := repeatErr.(*Error)
				if !ok || platformError.Exit != 5 || platformError.Code != "publish.curseforge_rejected" || repeated.Complete || repeated.Submission != "rejected" {
					t.Fatalf("rejected original upload lost business failure: %+v, %v", repeated, repeatErr)
				}
				if _, suggested := repeated.Data.(map[string]any)["nextCommand"]; suggested {
					t.Fatal("rejected upload suggests further verification")
				}
			}
		})
	}
}
func TestCurseForgePlanHasNoProfileOrHomeEffects(t *testing.T) {
	home := filepath.Join(t.TempDir(), "absent")
	raw, _ := json.Marshal(map[string]any{"project_id": 123, "file": publishZip(t), "changelog": "fix", "release_type": "release"})
	result, err := RunCurseForge(context.Background(), "plugin upload", Request{Home: home, Input: raw, Plan: true})
	if err != nil || !result.Complete {
		t.Fatalf("%+v %v", result, err)
	}
	if _, err = os.Stat(home); !os.IsNotExist(err) {
		t.Fatal("plan created home")
	}
}
func TestCurseForgePaginationRejectsIncompleteAndRepeatedRows(t *testing.T) {
	for _, scenario := range []string{"complete", "repeated", "total-changed", "missing-page"} {
		t.Run(scenario, func(t *testing.T) {
			calls := 0
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				calls++
				switch scenario {
				case "complete":
					io.WriteString(w, `{"data":[{"id":1,"name":"Addon"}],"pagination":{"index":0,"resultCount":1,"totalCount":1}}`)
				case "repeated":
					fmt.Fprintf(w, `{"data":[{"id":1}],"pagination":{"index":%d,"resultCount":1,"totalCount":2}}`, calls-1)
				case "total-changed":
					fmt.Fprintf(w, `{"data":[{"id":%d}],"pagination":{"index":%d,"resultCount":1,"totalCount":%d}}`, calls, calls-1, calls+1)
				case "missing-page":
					io.WriteString(w, `{"data":[],"pagination":{"totalCount":1}}`)
				}
			}))
			defer server.Close()
			c := &cfClient{http: NewHTTP(), core: server.URL, config: map[string]string{"CURSEFORGE_API_KEY": "query", "CURSEFORGE_AUTHOR_ID": "7"}}
			result, err := c.projects(context.Background(), nil)
			if scenario == "complete" {
				if err != nil || !result.Complete {
					t.Fatalf("%+v %v", result, err)
				}
			} else if err == nil {
				t.Fatal("invalid pagination accepted")
			}
		})
	}
}
