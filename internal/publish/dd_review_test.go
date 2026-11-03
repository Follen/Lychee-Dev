package publish

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"net/http/httptest"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestDDBackupDeleteRejectsCrossGameReference(t *testing.T) {
	api := &ddFake{fn: func(p string, q url.Values, _ any, _ bool) (map[string]any, error) {
		switch p {
		case "/backup/detail":
			return ddEnvelope(map[string]any{"sn": "b", "game_type": json.Number("1")}), nil
		case "/backup/list":
			return ddEnvelope([]any{map[string]any{"sn": "b", "game_type": json.Number("1")}}), nil
		case "/game_type/list":
			return ddEnvelope([]any{map[string]any{"game_type": json.Number("1")}, map[string]any{"game_type": json.Number("2")}}), nil
		case "/share/list":
			rows := []any{}
			if q.Get("game_type") == "2" {
				rows = append(rows, map[string]any{"sn": "s", "backup_sn": "b"})
			}
			return ddEnvelope(map[string]any{"list": rows, "total": len(rows)}), nil
		case "/share/detail":
			return ddEnvelope(map[string]any{"sn": "s", "backup_sn": "b"}), nil
		}
		t.Fatalf("unexpected %s", p)
		return nil, nil
	}}
	if _, _, err := ddPrepareForm(context.Background(), api, "backup", "delete", map[string]any{"sn": "b"}); err == nil {
		t.Fatal("cross-game backup reference was ignored")
	}
}

func TestDDBatchDeletionResumesAfterLostResponseWithoutReplay(t *testing.T) {
	present := map[string]bool{"a": true, "b": true}
	writes := map[string]int{}
	lost := true
	api := &ddFake{fn: func(p string, _ url.Values, body any, mutates bool) (map[string]any, error) {
		switch p {
		case "/game_type/list":
			return ddEnvelope([]any{map[string]any{"game_type": json.Number("1")}}), nil
		case "/share/list":
			return ddEnvelope(map[string]any{"list": []any{}, "total": 0}), nil
		case "/backup/list":
			rows := []any{}
			for _, id := range []string{"a", "b"} {
				if present[id] {
					rows = append(rows, map[string]any{"sn": id, "game_type": json.Number("1")})
				}
			}
			return ddEnvelope(rows), nil
		case "/backup/delete":
			id := ddText(ddMap(body)["sn"])
			writes[id]++
			delete(present, id)
			if lost {
				lost = false
				return nil, errors.New("lost response")
			}
			return ddEnvelope(nil), nil
		}
		t.Fatalf("unexpected route %s", p)
		return nil, nil
	}}
	req := Request{Home: t.TempDir(), RequestID: "batch"}
	in := map[string]any{"backup_ids": []any{"a", "b"}, "confirm_delete": true, "session": "retained-batch-session"}
	r, err := ddDeleteBackups(context.Background(), api, req, in, false, false)
	if err == nil || r.Complete || writes["a"] != 1 || writes["b"] != 0 {
		t.Fatal(r, err, writes)
	}
	assertDDRecoveryInput(t, req, "retained-batch-session")
	r, err = ddDeleteBackups(context.Background(), api, req, map[string]any{"request": "batch"}, true, true)
	if err == nil || r.Complete || writes["a"] != 1 || writes["b"] != 0 {
		t.Fatal("read-only verification submitted the next item", r, err, writes)
	}
	r, err = ddDeleteBackups(context.Background(), api, req, map[string]any{"request": "batch"}, true, false)
	if err != nil || !r.Complete || writes["a"] != 1 || writes["b"] != 1 {
		t.Fatal(r, err, writes)
	}
}

func TestDDBackupFinishRecoveryRetainsRequestedName(t *testing.T) {
	root := t.TempDir()
	file := filepath.Join(root, "Interface", "AddOns", "A", "A.lua")
	os.MkdirAll(filepath.Dir(file), 0700)
	os.WriteFile(file, []byte("return 1"), 0600)
	v, err := ddBackupGenerate(context.Background(), map[string]any{"root": root, "output": filepath.Join(t.TempDir(), "bundle"), "game_type": json.Number("1"), "unknown_addons": []any{"A"}})
	if err != nil {
		t.Fatal(err)
	}
	file = ddText(ddMap(v)["bundle_file"])
	opened, err := ddLoadBundle(file)
	if err != nil {
		t.Fatal(err)
	}
	manifest := ddClone(opened.bundle.Manifest)
	opened.close()
	manifest["sn"] = "created"
	manifest["name"] = "default"
	writes := map[string]int{}
	api := &ddFake{fn: func(p string, _ url.Values, b any, m bool) (map[string]any, error) {
		if m {
			writes[p]++
		}
		switch p {
		case "/game_type/list":
			return ddEnvelope([]any{map[string]any{"game_type": json.Number("1")}}), nil
		case "/backup/pre_create":
			return ddEnvelope(map[string]any{"sn": "created", "unknown_addon": map[string]any{"u_url": ""}}), nil
		case "/backup/finish_create":
			return nil, errors.New("lost finish response")
		case "/backup/modify_name":
			manifest["name"] = ddMap(b)["name"]
			return ddEnvelope(nil), nil
		case "/backup/detail":
			return ddEnvelope(manifest), nil
		case "/backup/list":
			return ddEnvelope([]any{map[string]any{"sn": "created"}}), nil
		}
		t.Fatalf("unexpected %s", p)
		return nil, nil
	}}
	req := Request{Home: t.TempDir(), RequestID: "recovery"}
	r, err := ddBackupUpload(context.Background(), api, req, map[string]any{"file": file, "name": "chosen"})
	if err == nil || r.Complete {
		t.Fatal(r, err)
	}
	saved, err := ddLoad(req.Home, req.RequestID)
	if err != nil {
		t.Fatal(err)
	}
	if saved.Expected["name"] != "chosen" {
		t.Fatal("requested name was not retained at first intent")
	}
	r, err = ddVerifyBackup(context.Background(), api, req.Home, saved, "operation verify")
	if err == nil || r.Complete {
		t.Fatal("default name incorrectly verified")
	}
	r, err = ddResumeBackup(context.Background(), api, req.Home, saved)
	if err != nil || !r.Complete || writes["/backup/pre_create"] != 1 || writes["/backup/finish_create"] != 1 || writes["/backup/modify_name"] != 1 {
		t.Fatal(r, err, writes)
	}
}

func TestDDPlanValidatesArtifactsWithoutInitializingHome(t *testing.T) {
	dir := t.TempDir()
	file := filepath.Join(dir, "bad.zip")
	os.WriteFile(file, []byte("not zip"), 0600)
	raw, _ := json.Marshal(map[string]any{"session": "task", "file": file, "kind": "archive"})
	home := filepath.Join(dir, "absent")
	_, err := RunDD(context.Background(), "plugin upload", Request{Home: home, Plan: true, Input: raw})
	if err == nil {
		t.Fatal("invalid archive accepted by plan")
	}
	if _, err = os.Stat(home); !os.IsNotExist(err) {
		t.Fatal("plan initialized home", err)
	}
}

func TestDDPrivateBackupGrantIsNotPlaintext(t *testing.T) {
	raw := []byte(`{"u_url":"https://example.com/object?secret=value"}`)
	encrypted, err := ddBackupProtect(raw, false)
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(string(encrypted), "secret=value") {
		t.Fatal("grant persisted plaintext")
	}
	actual, err := ddBackupProtect(encrypted, true)
	if err != nil || string(actual) != string(raw) {
		t.Fatal("grant recovery", err)
	}
}

func TestDDBackupUnknownPUTReadsOriginalObjectBeforeContinuation(t *testing.T) {
	root := t.TempDir()
	file := filepath.Join(root, "Interface", "AddOns", "A", "A.lua")
	os.MkdirAll(filepath.Dir(file), 0700)
	os.WriteFile(file, []byte("return 1"), 0600)
	v, err := ddBackupGenerate(context.Background(), map[string]any{"root": root, "output": filepath.Join(t.TempDir(), "bundle"), "game_type": json.Number("1"), "unknown_addons": []any{"A"}})
	if err != nil {
		t.Fatal(err)
	}
	file = ddText(ddMap(v)["bundle_file"])
	opened, err := ddLoadBundle(file)
	if err != nil {
		t.Fatal(err)
	}
	manifest := ddClone(opened.bundle.Manifest)
	original, err := io.ReadAll(opened.files["unknown_addon"])
	opened.close()
	if err != nil {
		t.Fatal(err)
	}
	manifest["sn"] = "created"
	puts, gets := 0, 0
	var stored []byte
	srv := httptest.NewTLSServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Header.Get("Authorization") != "" {
			t.Error("account header on object request")
		}
		switch r.Method {
		case "PUT":
			puts++
			stored, _ = io.ReadAll(r.Body)
			conn, _, e := w.(http.Hijacker).Hijack()
			if e != nil {
				t.Error(e)
				return
			}
			conn.Close()
		case "GET":
			gets++
			if r.URL.RawQuery != "" {
				t.Error("signed PUT query sent on public readback")
			}
			w.Write(stored)
		default:
			t.Error("unexpected object method", r.Method)
		}
	}))
	defer srv.Close()
	transport := http.DefaultTransport
	http.DefaultTransport = srv.Client().Transport
	defer func() { http.DefaultTransport = transport }()
	finishes := 0
	api := &ddFake{fn: func(p string, _ url.Values, _ any, _ bool) (map[string]any, error) {
		switch p {
		case "/game_type/list":
			return ddEnvelope([]any{map[string]any{"game_type": json.Number("1")}}), nil
		case "/backup/pre_create":
			return ddEnvelope(map[string]any{"sn": "created", "unknown_addon": map[string]any{"u_url": srv.URL + "/original?secret=value"}}), nil
		case "/backup/finish_create":
			finishes++
			return ddEnvelope(nil), nil
		case "/backup/detail":
			return ddEnvelope(manifest), nil
		case "/backup/list":
			return ddEnvelope([]any{map[string]any{"sn": "created"}}), nil
		}
		t.Fatalf("unexpected %s", p)
		return nil, nil
	}}
	req := Request{Home: t.TempDir(), RequestID: "put-recovery"}
	r, err := ddBackupUpload(context.Background(), api, req, map[string]any{"file": file})
	if err == nil || r.Complete || puts != 1 || finishes != 0 || !bytes.Equal(stored, original) {
		t.Fatal(r, err, puts, finishes)
	}
	saved, err := ddLoad(req.Home, req.RequestID)
	if err != nil {
		t.Fatal(err)
	}
	r, err = ddVerifyBackup(context.Background(), api, req.Home, saved, "operation verify")
	if err == nil || r.Complete {
		t.Fatal("metadata verified uncertain component without byte readback")
	}
	r, err = ddResumeBackup(context.Background(), api, req.Home, saved)
	if err != nil || !r.Complete || puts != 1 || gets != 1 || finishes != 1 {
		t.Fatal(r, err, puts, gets, finishes)
	}
}

func TestDDPlanDoesNotEchoAuthorContent(t *testing.T) {
	r, err := RunDD(context.Background(), "plugin edit", Request{Plan: true, Input: []byte(`{"session":"task","sn":"addon","html_desc":"private-body-marker"}`)})
	if err != nil {
		t.Fatal(err)
	}
	raw, _ := json.Marshal(r)
	if string(raw) == "" {
		t.Fatal("empty plan")
	}
	for _, v := range []any{r.Data.(map[string]any)["input"]} {
		if m, ok := v.(map[string]any); ok && m["html_desc"] == "private-body-marker" {
			t.Fatal("plan echoed original body")
		}
	}
}

func TestDDCreatePlanRejectsMissingCommercialDependencies(t *testing.T) {
	for _, resource := range []string{"plugin", "config", "wa"} {
		for _, field := range []string{"need_buy", "jump_room", "with_associate"} {
			t.Run(resource+"/"+field, func(t *testing.T) {
				spec, _ := DDInput(resource + " create")
				input := ddClone(spec.Example)
				if resource == "wa" {
					delete(input, "wa_file")
					raw, err := os.ReadFile(filepath.Join("dd", "testdata", "wa", "modern.txt"))
					if err != nil {
						t.Fatal(err)
					}
					input["content"] = string(raw)
				}
				input[field] = true
				raw, _ := json.Marshal(input)
				home := filepath.Join(t.TempDir(), "absent")
				result, err := RunDD(context.Background(), resource+" create", Request{Home: home, Plan: true, Input: raw})
				if err == nil || result.Complete {
					t.Fatal("plan accepted missing local commercial dependency")
				}
				var pe *Error
				want := map[string]string{"need_buy": "$.price_fen", "jump_room": "$.room_id", "with_associate": "$.associated_acts"}[field]
				if !errors.As(err, &pe) || pe.Path != want {
					t.Fatalf("unexpected admission error: %v", err)
				}
				switch field {
				case "need_buy":
					input["price_fen"], input["buy_life_type"] = 100, "seven_day"
				case "jump_room":
					input["room_id"] = "room-1"
				case "with_associate":
					input["associated_acts"] = []any{map[string]any{"sn": "addon-1", "act_type": "addon"}}
				}
				raw, _ = json.Marshal(input)
				result, err = RunDD(context.Background(), resource+" create", Request{Home: home, Plan: true, Input: raw})
				if err != nil || !result.Complete {
					t.Fatalf("complete local dependencies rejected: %v", err)
				}
				if _, err := os.Stat(home); !os.IsNotExist(err) {
					t.Fatal("plan initialized home", err)
				}
			})
		}
	}
}
