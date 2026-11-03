package publish

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func newBeeFixture(t *testing.T, handler http.HandlerFunc) *newBeeCreator {
	t.Helper()
	server := httptest.NewServer(handler)
	t.Cleanup(server.Close)
	return &newBeeCreator{http: NewHTTP(), api: server.URL, next: server.URL, metadata: server.URL + "/metadata", headers: http.Header{"Token": {"fixture"}}, access: "fixture"}
}
func newBeeReply(w http.ResponseWriter, data any) {
	_ = json.NewEncoder(w).Encode(map[string]any{"code": 1, "data": data})
}

func TestNewBeeCreatorEditPreservesNestedObjectsAndExplicitClear(t *testing.T) {
	detail := map[string]any{"id": 7, "t_cloudblackid": 9, "t_title": "old", "t_content": "old body", "t_content_format": 1, "t_intro": "intro", "pic_url": []any{}, "t_content_origin": 1, "t_sharing": 0, "t_link_to_channel": false, "t_subscribe_plan_level": 0, "t_price": 0, "t_time_range": "", "t_linked_mods": []any{map[string]any{"mod_id": 8, "server_extension": "keep"}}, "t_ignored_unknown_mods": []any{}, "t_ignored_materials": []any{}, "t_ignored_fronts": []any{}, "t_roleid": "", "t_selected_eml": []any{map[string]any{"layout": "raw", "extension": true}}}
	mutations := 0
	client := newBeeFixture(t, func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/creator/wow/share_config/details_aps":
			newBeeReply(w, detail)
		case "/creator/wow/share_config/update":
			mutations++
			var body map[string]any
			if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
				t.Error(err)
			}
			if body["title"] != "" || body["intro"] != "" {
				t.Errorf("explicit empty lost: %#v", body)
			}
			nested := body["linked_mods"].([]any)[0].(map[string]any)
			if nested["server_extension"] != "keep" {
				t.Errorf("lost nested field: %#v", nested)
			}
			layouts := body["selected_eml"].([]any)
			if layouts[0].(map[string]any)["extension"] != true {
				t.Error("lost layout extension")
			}
			for k, v := range body {
				detail[k] = v
			}
			detail["t_title"] = ""
			detail["t_intro"] = ""
			newBeeReply(w, map[string]any{"id": 7})
		default:
			t.Errorf("unexpected endpoint %s", r.URL.Path)
			http.Error(w, "unexpected", 500)
		}
	})
	command := newBeeCreatorCommands["config edit"]
	// Title is deliberately nonempty in the public schema; intro supports clearing.
	input, err := command.spec.Decode([]byte(`{"id":7,"intro":""}`))
	if err != nil {
		t.Fatal(err)
	}
	// Existing server title can be empty and must survive an unrelated edit.
	detail["t_title"] = ""
	result, err := client.run(context.Background(), "config edit", command, input, Request{Home: t.TempDir(), RequestID: "edit"})
	if err != nil || !result.Complete || mutations != 1 {
		t.Fatalf("%+v %v writes=%d", result, err, mutations)
	}
}

func TestNewBeeCreatorAuthHandoffKeepsSecretsOutOfResult(t *testing.T) {
	dir := t.TempDir()
	payload, _ := json.Marshal(map[string]any{"exp": time.Now().Add(time.Hour).Unix()})
	token := "header." + base64.RawURLEncoding.EncodeToString(payload) + ".signature"
	if err := os.WriteFile(filepath.Join(dir, "access-token"), []byte(token), 0600); err != nil {
		t.Fatal(err)
	}
	calls := 0
	client := newBeeFixture(t, func(w http.ResponseWriter, r *http.Request) {
		calls++
		switch r.URL.Path {
		case "/v3/user/auth2web":
			if r.Header.Get("Authorization") != "Bearer "+token {
				t.Error("missing desktop bearer")
			}
			newBeeReply(w, map[string]any{"code": "secret-code"})
		case "/v3/user/exchange_web_code":
			newBeeReply(w, map[string]any{"token": "secret-author", "jwtToken": "secret-jwt"})
		case "/v3/user/refresh_web_resource_token":
			if r.Header.Get("Token") != "secret-author" {
				t.Error("missing author token")
			}
			newBeeReply(w, map[string]any{"resource_token": "secret-resource"})
		default:
			t.Errorf("unexpected %s", r.URL.Path)
		}
	})
	client.headers = nil
	client.access = ""
	client.authDir = dir
	value, err := client.read(context.Background(), "session doctor", map[string]any{})
	if err != nil || calls != 3 {
		t.Fatalf("%v calls=%d", err, calls)
	}
	raw, _ := json.Marshal(value)
	if strings.Contains(string(raw), "secret") {
		t.Fatalf("credential leak: %s", raw)
	}
	if client.headers.Get("Authorization") != "Bearer secret-resource" {
		t.Fatal("resource token not retained")
	}
}

func TestNewBeeCreatorReferenceTypeReadbackAndEmptyReplacement(t *testing.T) {
	client := newBeeFixture(t, func(w http.ResponseWriter, r *http.Request) {
		newBeeReply(w, map[string]any{"items": []any{map[string]any{"type": 4, "id": 9}}})
	})
	input := map[string]any{"id": json.Number("7"), "references": []any{map[string]any{"type": json.Number("1"), "id": json.Number("9")}}}
	if _, err := client.verifyRelation(context.Background(), "plugin reference-set", input, nil); err == nil {
		t.Fatal("accepted wrong target namespace")
	}
	input["references"] = []any{map[string]any{"type": json.Number("3"), "id": json.Number("9")}}
	if _, err := client.verifyRelation(context.Background(), "plugin reference-set", input, nil); err != nil {
		t.Fatal(err)
	}
}

func TestNewBeeCreatorPlanNoAuthAndWARevision(t *testing.T) {
	result, err := RunNewBee(context.Background(), "creator wa upload", Request{Plan: true, RequestID: "r", Input: []byte(`{"id":1,"version":"1.2.3","wa_str":"private body","wa_log":"fix"}`)})
	if err != nil || !result.Complete {
		t.Fatalf("%+v %v", result, err)
	}
	for _, pair := range [][2]string{{"1.2.3", "1.2.3"}, {"1.2.2", "1.2.3"}, {"bad", "1.2.3"}} {
		if newBeeVersionGreater(pair[0], pair[1]) {
			t.Fatalf("accepted %v", pair)
		}
	}
	if !newBeeVersionGreater("1.3.0", "1.2.9") {
		t.Fatal("rejected increasing version")
	}
}

func TestNewBeeCreatorVerifyReadsOriginalWithoutResubmitting(t *testing.T) {
	writes := 0
	visible := false
	client := newBeeFixture(t, func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/creator/wow/mod_file/edit_changelog":
			writes++
			newBeeReply(w, map[string]any{})
		case "/creator/wow/mod_file/get_changelog":
			value := "old"
			if visible {
				value = "new"
			}
			newBeeReply(w, map[string]any{"file_id": 9, "changelog": value})
		default:
			t.Errorf("unexpected %s", r.URL.Path)
		}
	})
	command := newBeeCreatorCommands["plugin changelog-edit"]
	input, err := command.spec.Decode([]byte(`{"id":9,"changelog":"new"}`))
	if err != nil {
		t.Fatal(err)
	}
	request := Request{Home: t.TempDir(), RequestID: "original"}
	if _, err = client.run(context.Background(), "plugin changelog-edit", command, input, request); err == nil {
		t.Fatal("expected stale readback")
	}
	visible = true
	result, err := client.verifyOriginal(context.Background(), "plugin changelog-edit", input, request)
	if err != nil || !result.Complete {
		t.Fatalf("%+v %v", result, err)
	}
	if writes != 1 {
		t.Fatalf("replayed writes %d", writes)
	}
}
