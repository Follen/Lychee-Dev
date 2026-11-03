package publish

import (
	"archive/zip"
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestNewBeeSelectiveBackupArtifactsAndManifestBinding(t *testing.T) {
	dir := t.TempDir()
	game := filepath.Join(dir, "game")
	output := filepath.Join(dir, "bundle")
	for name, value := range map[string]string{"Interface/AddOns/Selected/Selected.toc": "toc", "Interface/AddOns/Unselected/secret.lua": "not selected", "WTF/Account/ACC/SavedVariables/A.lua": "common", "WTF/Account/ACC/Realm/Hero/AddOns.txt": "role", "WTF/Account/ACC/Realm/Hero/saved.bak": "excluded", "WTF/Account/ACC/Realm/Other/AddOns.txt": "other"} {
		path := filepath.Join(game, filepath.FromSlash(name))
		if err := os.MkdirAll(filepath.Dir(path), 0700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(path, []byte(value), 0600); err != nil {
			t.Fatal(err)
		}
	}
	raw, _ := json.Marshal(map[string]any{"game_dir": game, "output_dir": output, "game_version_id": 1, "unknown_plugins": []string{"Selected"}, "wtf_accounts": []any{map[string]any{"account": "ACC", "include_common": true, "roles": []any{map[string]any{"server": "Realm", "name": "Hero"}}}}})
	input, err := newBeeCreatorCommands["backup pack"].spec.Decode(raw)
	if err != nil {
		t.Fatal(err)
	}
	value, err := newBeePack(context.Background(), input)
	if err != nil {
		t.Fatal(err)
	}
	path := value.(map[string]any)["bundle_file"].(string)
	bundle, err := newBeeReadBundle(path)
	if err != nil {
		t.Fatal(err)
	}
	if len(bundle.Artifacts) != 3 {
		t.Fatalf("artifacts=%d", len(bundle.Artifacts))
	}
	for _, artifact := range bundle.Artifacts {
		archive, err := zip.OpenReader(filepath.Join(output, artifact.File))
		if err != nil {
			t.Fatal(err)
		}
		for _, entry := range archive.File {
			if strings.Contains(entry.Name, "Other") || strings.Contains(entry.Name, "Unselected") || strings.HasSuffix(entry.Name, ".bak") {
				t.Errorf("unselected data %s", entry.Name)
			}
		}
		archive.Close()
	}
	bundle.Manifest["t_unKnown_list"] = []any{map[string]any{"name": "Different"}}
	raw, _ = json.Marshal(bundle)
	if err = os.WriteFile(path, raw, 0600); err != nil {
		t.Fatal(err)
	}
	if _, err = newBeeReadBundle(path); err == nil {
		t.Fatal("manifest selection was not bound to archive contents")
	}
}

func TestNewBeeBackupDeleteResumesCompletedItemsWithoutReplay(t *testing.T) {
	present := map[int64]bool{1: true, 2: true}
	deletes := map[int64]int{}
	secondFails := true
	client := newBeeFixture(t, func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/v3/share/list":
			rows := []any{}
			for _, id := range []int64{1, 2} {
				if present[id] {
					rows = append(rows, map[string]any{"t_id": id, "t_name": "backup"})
				}
			}
			newBeeReply(w, rows)
		case "/creator/wow/share_config/publish_list":
			newBeeReply(w, map[string]any{"list": []any{}, "total": 0})
		case "/v3/share/delete":
			var body map[string]json.Number
			_ = json.NewDecoder(r.Body).Decode(&body)
			id, _ := body["t_id"].Int64()
			deletes[id]++
			delete(present, id)
			if id == 2 && secondFails {
				secondFails = false
				http.Error(w, "lost response", 500)
				return
			}
			newBeeReply(w, map[string]any{})
		default:
			t.Errorf("unexpected %s", r.URL.Path)
			http.Error(w, "bad", 500)
		}
	})
	input, err := newBeeCreatorCommands["backup delete"].spec.Decode([]byte(`{"ids":[1,2],"confirm_delete":true}`))
	if err != nil {
		t.Fatal(err)
	}
	request := Request{Home: t.TempDir(), RequestID: "delete"}
	if _, err = client.runBackupDelete(context.Background(), input, request); err == nil {
		t.Fatal("expected lost response")
	}
	result, err := client.runBackupDelete(context.Background(), input, request)
	if err != nil || !result.Complete {
		t.Fatalf("%+v %v", result, err)
	}
	if deletes[1] != 1 || deletes[2] != 1 {
		t.Fatalf("replayed deletes: %v", deletes)
	}
}

func TestNewBeeBackupReferencePaginationFailsClosed(t *testing.T) {
	client := newBeeFixture(t, func(w http.ResponseWriter, r *http.Request) {
		newBeeReply(w, map[string]any{"list": []any{map[string]any{"id": 7, "cloud_id": 3}}, "total": 2, "next_offset": 0})
	})
	if _, err := client.backupReferences(context.Background()); err == nil {
		t.Fatal("non-advancing reference cursor accepted")
	}
}

func TestNewBeeBackupManifestRetainsKnownPluginIdentity(t *testing.T) {
	if newBeeContains([]any{map[string]any{"id": 1, "name": "Same"}}, []any{map[string]any{"id": 2, "name": "Same"}}) {
		t.Fatal("known plugin identity ignored")
	}
}

func TestNewBeeBackupUploadResumesLostCreateWithoutUploadingAgain(t *testing.T) {
	dir := t.TempDir()
	game := filepath.Join(dir, "game")
	source := filepath.Join(game, "Interface", "AddOns", "One")
	if err := os.MkdirAll(source, 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(source, "One.toc"), []byte("toc"), 0600); err != nil {
		t.Fatal(err)
	}
	input := map[string]any{"game_dir": game, "output_dir": filepath.Join(dir, "bundle"), "game_version_id": json.Number("1"), "unknown_plugins": []any{"One"}}
	packed, err := newBeePack(context.Background(), input)
	if err != nil {
		t.Fatal(err)
	}
	bundlePath := packed.(map[string]any)["bundle_file"].(string)
	rows := []any{}
	puts, creates := 0, 0
	var server *httptest.Server
	server = httptest.NewTLSServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/v3/share/list":
			newBeeReply(w, rows)
		case "/metadata":
			_ = json.NewEncoder(w).Encode(map[string]any{"game_version": []any{map[string]any{"id": 1}}})
		case "/share/generate_upload_url_v2":
			newBeeReply(w, map[string]any{"file_path": "object/one", "upload_url": server.URL + "/blob?signature=secret"})
		case "/blob":
			puts++
			if r.Header.Get("Authorization") != "" || r.Header.Get("Token") != "" {
				t.Error("creator credentials crossed into object storage")
			}
			w.WriteHeader(200)
		case "/v3/share/create":
			creates++
			rows = []any{map[string]any{"t_id": 11, "t_Versionid": 1, "t_Known_plug": []any{}, "t_unKnown_list": []any{map[string]any{"name": "One"}}, "t_material_list": []any{}, "t_font_list": []any{}, "t_unKnown_plug": "object/one", "t_material": nil, "t_font_ncs_hash": nil, "wtflist": []any{}}}
			http.Error(w, "response lost", 500)
		default:
			t.Errorf("unexpected %s", r.URL.Path)
			http.Error(w, "unexpected", 500)
		}
	}))
	defer server.Close()
	client := &newBeeCreator{http: &HTTP{client: server.Client()}, api: server.URL, next: server.URL, metadata: server.URL + "/metadata", headers: http.Header{"Token": {"fixture"}}, access: "fixture"}
	request := Request{Home: filepath.Join(dir, "home"), RequestID: "original"}
	upload := map[string]any{"bundle_file": bundlePath}
	if _, err = client.runBackupUpload(context.Background(), upload, request); err == nil {
		t.Fatal("expected lost create response")
	}
	client.access = "switched-session"
	if result, err := client.runBackupUpload(context.Background(), upload, request); err == nil || result.Complete {
		t.Fatalf("switched session recovered backup: %+v %v", result, err)
	}
	client.access = "fixture"
	result, err := client.runBackupUpload(context.Background(), upload, request)
	if err != nil || !result.Complete {
		t.Fatalf("%+v %v", result, err)
	}
	if puts != 1 || creates != 1 {
		t.Fatalf("replayed upload/create: %d/%d", puts, creates)
	}
}
