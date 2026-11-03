package publish

import (
	"archive/zip"
	"context"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"net/http/httptest"
	"net/url"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"testing"
)

func modusBackupTestInput(t *testing.T, action string, v map[string]any) map[string]any {
	t.Helper()
	spec, ok := modusBackupInput(action)
	if !ok {
		t.Fatal(action)
	}
	raw, _ := json.Marshal(v)
	in, err := spec.Decode(raw)
	if err != nil {
		t.Fatal(err)
	}
	return in
}
func modusBackupTestFiles(t *testing.T) (string, string, map[string]any) {
	t.Helper()
	base := t.TempDir()
	root := filepath.Join(base, "game")
	out := filepath.Join(base, "backup")
	files := map[string]string{"Interface/AddOns/Selected/Selected.toc": "## Interface: 120105", "Interface/AddOns/Selected/a.lua": "selected", "Interface/AddOns/Other/o.lua": "unselected", "Fonts/test.ttf": "font-fixture", "Interface/Art/x.blp": "art", "WTF/Config.wtf": "config", "WTF/Account/ACC/SavedVariables/Common.lua": "common", "WTF/Account/ACC/SavedVariables/BigFootSync.lua": "excluded", "WTF/Account/ACC/Realm/Hero/Config-cache.wtf": "hero", "WTF/Account/ACC/Realm/Hero/SavedVariables/A.lua": "hero lua", "WTF/Account/ACC/Realm/Hero/SavedVariables/A.lua.bak": "excluded backup", "WTF/Account/ACC/Realm/Other/SavedVariables/B.lua": "other"}
	for rel, content := range files {
		file := filepath.Join(root, filepath.FromSlash(rel))
		if err := os.MkdirAll(filepath.Dir(file), 0700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(file, []byte(content), 0600); err != nil {
			t.Fatal(err)
		}
	}
	in := modusBackupTestInput(t, "backup generate", map[string]any{"server_type": 0, "game_root": root, "output_dir": out, "backup_name": "Example", "addon_names": []any{"Selected"}, "font_names": []any{"test.ttf"}, "material_names": []any{"Art"}, "roles": []any{map[string]any{"account": "ACC", "server": "Realm", "role": "Hero"}}})
	return root, out, in
}
func TestModusBackupSelectiveNativeManifestAndInjectedRole(t *testing.T) {
	_, out, in := modusBackupTestFiles(t)
	ctx := context.Background()
	v, err := generateModusBackup(ctx, in, false)
	if err != nil {
		t.Fatal(err)
	}
	manifest := v.(map[string]any)["manifest_file"].(string)
	for _, absent := range []string{"Interface/AddOns/Other", "WTF/Account/ACC/Realm/Other", "WTF/Account/ACC/SavedVariables/BigFootSync.lua", "WTF/Account/ACC/Realm/Hero/SavedVariables/A.lua.bak"} {
		if _, err := os.Stat(filepath.Join(out, filepath.FromSlash(absent))); !os.IsNotExist(err) {
			t.Fatalf("unselected file copied: %s", absent)
		}
	}
	m, objects, err := loadModusBackupManifest(ctx, manifest, 0)
	if err != nil {
		t.Fatal(err)
	}
	if m["schemaVersion"] != "config-backup.v2" || len(m["sections"].([]any)) != 6 || len(objects) != 4 {
		t.Fatalf("invalid native manifest or object plan: %d", len(objects))
	}
	for _, o := range objects {
		if o.Section == "wtf" {
			names := map[string]bool{}
			for _, entry := range o.Entries {
				names[entry.Name] = true
			}
			if !names["ACC/Realm/Hero/Config-cache.wtf"] || !names["ACC/SavedVariables/Common.lua"] || !names["Config.wtf"] {
				t.Fatalf("lost native WTF layout: %v", names)
			}
		}
	}
	rogue := filepath.Join(out, "WTF", "Account", "ACC", "Realm", "Other", "A.lua")
	os.MkdirAll(filepath.Dir(rogue), 0700)
	os.WriteFile(rogue, []byte("unselected"), 0600)
	if _, _, err = loadModusBackupManifest(ctx, manifest, 0); err == nil {
		t.Fatal("injected unselected character entered upload plan")
	}
}
func TestModusBackupPlansDoNotCreateFilesOrReadAccount(t *testing.T) {
	_, out, in := modusBackupTestFiles(t)
	home := filepath.Join(t.TempDir(), "absent-home")
	r, err := runModusBackup(context.Background(), "backup generate", Request{Home: home, Plan: true}, in, nil)
	if err != nil || !r.Complete {
		t.Fatal(err)
	}
	if _, err = os.Stat(out); !os.IsNotExist(err) {
		t.Fatal("plan created output")
	}
	if _, err = os.Stat(home); !os.IsNotExist(err) {
		t.Fatal("plan initialized home")
	}
	for _, action := range modusBackupActions() {
		spec, _ := modusBackupInput(action)
		raw, _ := json.Marshal(spec.Example)
		if _, err := spec.Decode(raw); err != nil {
			t.Errorf("invalid %s example: %v", action, err)
		}
	}
}
func TestModusBackupManifestRejectsRolePathAndDuplicateJSON(t *testing.T) {
	_, _, in := modusBackupTestFiles(t)
	v, err := generateModusBackup(context.Background(), in, false)
	if err != nil {
		t.Fatal(err)
	}
	filename := v.(map[string]any)["manifest_file"].(string)
	raw, _ := os.ReadFile(filename)
	var manifest map[string]any
	json.Unmarshal(raw, &manifest)
	for _, v := range manifest["sections"].([]any) {
		section := v.(map[string]any)
		if section["key"] == "wtf" {
			for _, v := range section["items"].([]any) {
				item := v.(map[string]any)
				if item["name"] == "ACC" {
					item["targetRelativePath"] = "WTF/Account/Other"
				}
			}
		}
	}
	modified, _ := json.Marshal(manifest)
	os.WriteFile(filename, modified, 0600)
	if _, _, err = loadModusBackupManifest(context.Background(), filename, 0); err == nil {
		t.Fatal("accepted mismatched account path")
	}
	os.WriteFile(filename, []byte(`{"schemaVersion":"config-backup.v2","schemaVersion":"config-backup.v2"}`), 0600)
	if _, _, err = loadModusBackupManifest(context.Background(), filename, 0); err == nil {
		t.Fatal("accepted duplicate manifest fields")
	}
}
func TestModusBackupCrossBuildReferenceAndUnknownBatchDelete(t *testing.T) {
	backups := map[string]bool{"1": true, "2": true, "3": true}
	deletes := map[string]int{}
	reference := true
	lost := true
	shareBuilds := map[int]bool{}
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch {
		case r.URL.Path == "/system/user/getInfo":
			w.Write([]byte(`{"data":{"user":{"userId":7}}}`))
		case r.URL.Path == "/system/user/share/list":
			var body map[string]any
			json.NewDecoder(r.Body).Decode(&body)
			build := int(body["server"].(float64))
			shareBuilds[build] = true
			if reference && build == 4 {
				w.Write([]byte(`{"data":{"rows":[{"id":"share-4"}],"total":1}}`))
			} else {
				w.Write([]byte(`{"data":{"rows":[],"total":0}}`))
			}
		case r.URL.Path == "/system/user/share/detail":
			w.Write([]byte(`{"data":[{"id":"share-4","backupId":1,"title":"uses backup"}]}`))
		case r.URL.Path == "/system/user/backup/list":
			rows := []any{}
			for _, id := range []string{"1", "2", "3"} {
				if backups[id] {
					rows = append(rows, map[string]any{"id": json.Number(id), "backupName": "backup " + id, "serverType": 0})
				}
			}
			json.NewEncoder(w).Encode(map[string]any{"data": rows})
		case strings.HasPrefix(r.URL.Path, "/system/user/backup/detail/"):
			id := strings.TrimPrefix(r.URL.Path, "/system/user/backup/detail/")
			if backups[id] {
				json.NewEncoder(w).Encode(map[string]any{"data": map[string]any{"id": json.Number(id), "backupName": "backup " + id, "serverType": 0}})
			} else {
				w.WriteHeader(404)
			}
		case strings.HasPrefix(r.URL.Path, "/system/user/backup/delete/"):
			id := strings.TrimPrefix(r.URL.Path, "/system/user/backup/delete/")
			deletes[id]++
			delete(backups, id)
			if id == "2" && lost {
				lost = false
				w.WriteHeader(503)
			} else {
				w.Write([]byte(`{"code":200,"data":true}`))
			}
		default:
			t.Errorf("unexpected path %s", r.URL.Path)
			w.WriteHeader(404)
		}
	}))
	defer srv.Close()
	c := &modusClient{http: NewHTTP(), origin: srv.URL + "/", main: true, token: "fixture"}
	ctx := context.Background()
	in := modusBackupTestInput(t, "backup delete", map[string]any{"server_type": 0, "backup_ids": []any{1}, "confirm_delete": true})
	r := Request{Home: t.TempDir(), RequestID: "protected"}
	result, err := runModusBackup(ctx, "backup delete", r, in, c)
	if err == nil || result.Complete || deletes["1"] != 0 || len(shareBuilds) != 5 {
		t.Fatalf("cross-Build protection failed: %+v %v %v", result, err, shareBuilds)
	}
	reference = false
	in = modusBackupTestInput(t, "backup delete", map[string]any{"server_type": 0, "backup_ids": []any{1, 2, 3}, "confirm_delete": true})
	r.RequestID = "delete-all"
	if _, err = runModusBackup(ctx, "backup delete", r, in, c); err == nil {
		t.Fatal("lost deletion acknowledgement hidden")
	}
	if deletes["1"] != 1 || deletes["2"] != 1 || deletes["3"] != 0 {
		t.Fatal(deletes)
	}
	recovery := modusBackupTestInput(t, "backup verify", map[string]any{"request": "delete-all"})
	if _, err = runModusBackup(ctx, "backup verify", r, recovery, c); err == nil || deletes["3"] != 0 {
		t.Fatal("verify sent pending mutation")
	}
	result, err = runModusBackup(ctx, "backup resume", r, recovery, c)
	if err != nil || !result.Complete || deletes["1"] != 1 || deletes["2"] != 1 || deletes["3"] != 1 {
		t.Fatalf("original batch did not resume: %+v %v %v", result, err, deletes)
	}
}

type modusBackupTestTransport func(*http.Request) (*http.Response, error)

func (f modusBackupTestTransport) RoundTrip(r *http.Request) (*http.Response, error) { return f(r) }
func TestModusBackupUploadNativeProtocolAndLostRegistration(t *testing.T) {
	_, _, in := modusBackupTestFiles(t)
	ctx := context.Background()
	generated, err := generateModusBackup(ctx, in, false)
	if err != nil {
		t.Fatal(err)
	}
	manifest := generated.(map[string]any)["manifest_file"].(string)
	objects := map[string][]byte{}
	signatures, puts, storage, creates := 0, 0, 0, 0
	lostPut := true
	account := 7
	var cloud map[string]any
	var uploadBody map[string]any
	srv := httptest.NewTLSServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if strings.HasPrefix(r.URL.Path, "/objects/") {
			if r.Header.Get("Authorization") != "" || r.Header.Get("X-Device-Id") != "" {
				t.Error("credentials leaked to object store")
			}
			key := strings.TrimPrefix(r.URL.Path, "/")
			if r.Method == "PUT" {
				puts++
				objects[key], _ = io.ReadAll(r.Body)
				if lostPut {
					lostPut = false
					w.WriteHeader(503)
				} else {
					w.WriteHeader(204)
				}
			} else if raw, ok := objects[key]; ok {
				w.Write(raw)
			} else {
				w.WriteHeader(404)
			}
			return
		}
		switch r.URL.Path {
		case "/system/user/getInfo":
			json.NewEncoder(w).Encode(map[string]any{"data": map[string]any{"user": map[string]any{"userId": account}}})
		case "/game/data/file/upload/check/font":
			var body map[string]any
			json.NewDecoder(r.Body).Decode(&body)
			json.NewEncoder(w).Encode(map[string]any{"data": map[string]any{"notExist": body["fontMd5"]}})
		case "/game/data/file/getSignature":
			signatures++
			key := "objects/" + strconv.Itoa(signatures)
			if r.URL.Query().Get("sign") == "" {
				t.Error("missing platform signature")
			}
			json.NewEncoder(w).Encode(map[string]any{"data": map[string]any{"key": key, "signedUrl": "https://storage.example/" + key + "?secret=fixture"}})
		case "/game/data/file/upload/data/storage":
			storage++
			w.Write([]byte(`{"data":true}`))
		case "/system/user/backup/list":
			rows := []any{}
			if cloud != nil {
				rows = append(rows, cloud)
			}
			json.NewEncoder(w).Encode(map[string]any{"data": rows})
		case "/system/user/backup/create":
			creates++
			d := json.NewDecoder(r.Body)
			d.UseNumber()
			d.Decode(&uploadBody)
			cloud = map[string]any{}
			for k, v := range uploadBody {
				cloud[k] = v
			}
			cloud["id"] = json.Number("9")
			w.WriteHeader(503)
		case "/system/user/backup/detail/9":
			json.NewEncoder(w).Encode(map[string]any{"data": cloud})
		default:
			t.Errorf("unexpected path %s", r.URL.Path)
			w.WriteHeader(404)
		}
	}))
	defer srv.Close()
	hc := srv.Client()
	transport := hc.Transport
	target, _ := url.Parse(srv.URL)
	hc.Transport = modusBackupTestTransport(func(r *http.Request) (*http.Response, error) {
		copy := r.Clone(r.Context())
		u := *r.URL
		copy.URL = &u
		if u.Host == "storage.example" || u.Host == "cdn.modus.cool" {
			copy.URL.Host = target.Host
			copy.URL.Scheme = target.Scheme
		}
		return transport.RoundTrip(copy)
	})
	c := &modusClient{http: &HTTP{client: hc}, origin: srv.URL + "/", main: true, token: "fixture-secret", device: "fixture-device"}
	r := Request{Home: t.TempDir(), RequestID: "native-backup"}
	upload := modusBackupTestInput(t, "backup upload", map[string]any{"server_type": 0, "manifest_file": manifest})
	result, err := runModusBackup(ctx, "backup upload", r, upload, c)
	if err == nil || result.Complete {
		t.Fatal("unknown object upload hidden")
	}
	if signatures != 1 || puts != 1 || storage != 0 || creates != 0 {
		t.Fatalf("unknown object continued writes: %d %d %d %d", signatures, puts, storage, creates)
	}
	recovery := modusBackupTestInput(t, "backup resume", map[string]any{"request": r.RequestID})
	account = 8
	if _, err = runModusBackup(ctx, "backup resume", r, recovery, c); err == nil || puts != 1 {
		t.Fatal("foreign account resumed original upload")
	}
	account = 7
	result, err = runModusBackup(ctx, "backup resume", r, recovery, c)
	if err == nil || result.Complete {
		t.Fatal("unknown registration hidden")
	}
	if signatures != 5 || puts != 5 || storage != 5 || creates != 1 {
		t.Fatalf("wrong upload stages %d %d %d %d", signatures, puts, storage, creates)
	}
	s, err := readModusBackup(r.Home, r.RequestID)
	if err != nil || s.Phase != "register-intent" {
		t.Fatalf("state %s %v", s.Phase, err)
	}
	serialized, _ := json.Marshal(s)
	if strings.Contains(string(serialized), "secret=fixture") || strings.Contains(string(serialized), "fixture-secret") {
		t.Fatal("signed URL or account credential persisted")
	}
	for _, o := range s.Objects {
		if o.Section == "wtf" {
			z, err := zip.OpenReader(o.File)
			if err != nil {
				t.Fatal(err)
			}
			names := map[string]bool{}
			for _, file := range z.File {
				names[file.Name] = true
			}
			z.Close()
			if !names["ACC/Realm/Hero/Config-cache.wtf"] || names["ACC/Realm/Other/SavedVariables/B.lua"] {
				t.Fatal(names)
			}
		}
	}
	result, err = runModusBackup(ctx, "backup resume", r, recovery, c)
	if err != nil || !result.Complete || creates != 1 || puts != 5 {
		t.Fatalf("registration recovery failed: %+v %v counts %d %d", result, err, creates, puts)
	}
	var linked map[string]any
	raw, _ := os.ReadFile(manifest)
	json.Unmarshal(raw, &linked)
	if linked["source"].(map[string]any)["cloudBackupId"] != "9" {
		t.Fatal("local manifest not linked")
	}
	result, err = runModusBackup(ctx, "backup resume", r, recovery, c)
	if err != nil || !result.Complete || creates != 1 {
		t.Fatal("completed backup replayed")
	}
	if uploadBody["manifestPath"] == "" || uploadBody["wtfNum"] != json.Number("1") || uploadBody["fontNum"] != json.Number("1") {
		t.Fatalf("native registration body incomplete: %v", uploadBody)
	}
}
func TestModusBackupDeletePreviewRequiresCompleteReferencePagination(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path == "/system/user/backup/list" {
			w.Write([]byte(`{"data":[{"id":1,"serverType":0,"backupName":"Example"}]}`))
		} else if r.URL.Path == "/system/user/share/list" {
			w.Write([]byte(`{"data":{"rows":[],"total":1}}`))
		} else {
			t.Errorf("unexpected %s", r.URL.Path)
		}
	}))
	defer srv.Close()
	c := &modusClient{http: NewHTTP(), origin: srv.URL + "/", main: true}
	_, err := previewModusBackup(context.Background(), c, modusBackupTestInput(t, "backup delete-preview", map[string]any{"server_type": 0, "backup_ids": []any{1}}))
	var e *Error
	if !errors.As(err, &e) || e.Code != "publish.modus_backup_protection" {
		t.Fatalf("incomplete protection admitted: %v", err)
	}
}

func TestModusBackupRetainedArtifactAndUnknownStorageAreNotReplayed(t *testing.T) {
	for _, fault := range []string{"signature", "storage"} {
		t.Run(fault, func(t *testing.T) {
			_, _, in := modusBackupTestFiles(t)
			in["font_names"] = []any{}
			in["material_names"] = []any{}
			in["roles"] = []any{}
			generated, err := generateModusBackup(context.Background(), in, false)
			if err != nil {
				t.Fatal(err)
			}
			manifest := generated.(map[string]any)["manifest_file"].(string)
			signatures, puts, registrations := 0, 0, 0
			var base string
			srv := httptest.NewTLSServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				switch r.URL.Path {
				case "/system/user/getInfo":
					w.Write([]byte(`{"data":{"user":{"userId":7}}}`))
				case "/game/data/file/getSignature":
					signatures++
					if fault == "signature" {
						w.WriteHeader(503)
						return
					}
					json.NewEncoder(w).Encode(map[string]any{"data": map[string]any{"key": "object", "signedUrl": base + "/object"}})
				case "/object":
					puts++
					io.Copy(io.Discard, r.Body)
					w.WriteHeader(204)
				case "/game/data/file/upload/data/storage":
					registrations++
					w.WriteHeader(503)
				default:
					t.Errorf("unexpected %s", r.URL.Path)
					w.WriteHeader(404)
				}
			}))
			defer srv.Close()
			base = srv.URL
			c := &modusClient{http: &HTTP{client: srv.Client()}, origin: base + "/", main: true, token: "fixture"}
			request := Request{Home: t.TempDir(), RequestID: "original"}
			upload := modusBackupTestInput(t, "backup upload", map[string]any{"server_type": 0, "manifest_file": manifest})
			if _, err = runModusBackup(context.Background(), "backup upload", request, upload, c); err == nil {
				t.Fatal("injected failure hidden")
			}
			state, err := readModusBackup(request.Home, request.RequestID)
			if err != nil {
				t.Fatal(err)
			}
			if fault == "signature" {
				if err = os.WriteFile(state.Objects[0].File, []byte("changed original"), 0600); err != nil {
					t.Fatal(err)
				}
			}
			recovery := modusBackupTestInput(t, "backup resume", map[string]any{"request": request.RequestID})
			result, err := runModusBackup(context.Background(), "backup resume", request, recovery, c)
			if err == nil || result.Complete || signatures != 1 {
				t.Fatalf("original action replayed: %+v %v signatures=%d", result, err, signatures)
			}
			if fault == "storage" {
				var e *Error
				if !errors.As(err, &e) || e.Exit != 6 || puts != 1 || registrations != 1 {
					t.Fatalf("unknown storage replayed: %v %d %d", err, puts, registrations)
				}
			} else if puts != 0 || registrations != 0 {
				t.Fatal("changed artifact uploaded")
			}
		})
	}
}
