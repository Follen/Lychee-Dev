package publish

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"testing"
)

func TestNewBeePluginUploadPreservesChannelAndNormalizesBuildReadback(t *testing.T) {
	for _, explicit := range []bool{false, true} {
		t.Run(map[bool]string{false: "omitted", true: "explicit-false"}[explicit], func(t *testing.T) {
			file := filepath.Join(t.TempDir(), "Addon.zip")
			if err := os.WriteFile(file, []byte("archive"), 0600); err != nil {
				t.Fatal(err)
			}
			writes := 0
			client := newBeeFixture(t, func(w http.ResponseWriter, r *http.Request) {
				switch r.URL.Path {
				case "/creator/wow/mod/publish_detail":
					newBeeReply(w, map[string]any{"id": 7, "t_link_to_channel": true})
				case "/metadata":
					_ = json.NewEncoder(w).Encode(map[string]any{"game_version": []any{map[string]any{"version": "120100"}}})
				case "/creator/wow/mod_file/mod_file_list":
					rows := []any{}
					if writes > 0 {
						rows = append(rows, map[string]any{"t_display_name": "1.0.0", "versions": []any{map[string]any{"version": "120100"}}})
					}
					newBeeReply(w, map[string]any{"list": rows})
				case "/creator/wow/mod_file/upload_mod_file":
					writes++
					if err := r.ParseMultipartForm(1 << 20); err != nil {
						t.Error(err)
					}
					want := "true"
					if explicit {
						want = "false"
					}
					if got := r.FormValue("link_to_channel"); got != want {
						t.Errorf("link_to_channel=%q want %q", got, want)
					}
					newBeeReply(w, map[string]any{"id": 9})
				default:
					t.Errorf("unexpected endpoint %s", r.URL.Path)
					http.Error(w, "unexpected", 500)
				}
			})
			input := map[string]any{"id": json.Number("7"), "file": file, "version": "1.0.0", "game_version_list": []any{"120100"}}
			if explicit {
				input["link_to_channel"] = false
			}
			result, err := client.run(context.Background(), "plugin upload", newBeeCreatorCommands["plugin upload"], input, Request{Home: t.TempDir(), RequestID: "original"})
			if err != nil || !result.Complete || writes != 1 {
				t.Fatalf("result=%+v writes=%d err=%v", result, writes, err)
			}
		})
	}
}

func TestNewBeePluginUploadRejectsUnsupportedArchiveInPlan(t *testing.T) {
	for _, name := range []string{"Addon.txt", "Addon.zip"} {
		t.Run(name, func(t *testing.T) {
			file := filepath.Join(t.TempDir(), name)
			f, err := os.Create(file)
			if err != nil {
				t.Fatal(err)
			}
			size := int64(1)
			if name == "Addon.zip" {
				size = 300*1024*1024 + 1
			}
			if err = f.Truncate(size); err != nil {
				t.Fatal(err)
			}
			f.Close()
			raw, _ := json.Marshal(map[string]any{"id": 7, "file": file, "version": "1.0.0", "game_version_list": []any{"120100"}})
			_, err = RunNewBee(context.Background(), "creator plugin upload", Request{Plan: true, RequestID: "original", Input: raw})
			if err == nil {
				t.Fatal("unsupported plugin archive passed local admission")
			}
		})
	}
}

func TestNewBeePluginUploadReadbackRejectsMissingBuild(t *testing.T) {
	client := newBeeFixture(t, func(w http.ResponseWriter, r *http.Request) {
		newBeeReply(w, map[string]any{"list": []any{map[string]any{"t_display_name": "1.0.0", "versions": []any{map[string]any{"build": "11509"}}}}})
	})
	input := map[string]any{"id": json.Number("7"), "version": "1.0.0", "game_version_list": []any{"120100"}}
	if _, err := client.verify(context.Background(), "plugin upload", input, nil, nil, nil); err == nil {
		t.Fatal("accepted a version without the requested game build")
	}
}

func TestNewBeeBackupUploadRejectsArtifactChangedAfterBundleCheck(t *testing.T) {
	file := filepath.Join(t.TempDir(), "original.zip")
	if err := os.WriteFile(file, []byte("original"), 0600); err != nil {
		t.Fatal(err)
	}
	sum := sha256.Sum256([]byte("original"))
	puts := 0
	var server *httptest.Server
	server = httptest.NewTLSServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method == http.MethodPut {
			puts++
			w.WriteHeader(200)
			return
		}
		if err := os.WriteFile(file, []byte("modified"), 0600); err != nil {
			t.Error(err)
		}
		newBeeReply(w, map[string]any{"file_path": "object", "upload_url": server.URL + "/object"})
	}))
	defer server.Close()
	client := &newBeeCreator{http: &HTTP{client: server.Client()}, api: server.URL, next: server.URL, headers: http.Header{"Token": {"fixture"}}, access: "fixture"}
	_, err := client.uploadBackupObject(context.Background(), file, newBeeBackupArtifact{Size: 8, SHA256: hex.EncodeToString(sum[:]), FileType: "unknown_plug"})
	if err == nil || puts != 0 {
		t.Fatalf("changed backup submitted: puts=%d err=%v", puts, err)
	}
}
