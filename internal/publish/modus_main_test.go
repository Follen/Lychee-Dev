package publish

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

func TestModusMainPatchPreservesAndClearsWithOriginalIdentity(t *testing.T) {
	current := map[string]any{"id": "wa-one", "addonsId": "8", "content": "<p>Old</p>", "imageUrl": "https://cdn.modus.cool/image.webp", "isPaid": 0, "isPublic": 1, "price": 0, "shareType": 0, "tags": "1", "title": "Keep", "filePath": "", "supportAddon": "WeakAuras", "version": "1", "requiredTierId": 5, "platform": 1, "synchronizationType": 1}
	writes := 0
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if strings.HasSuffix(r.URL.Path, ".json") {
			if r.Header.Get("Authorization") != "" {
				t.Error("credentials sent to public options")
			}
			if strings.Contains(r.URL.Path, "support") {
				w.Write([]byte(`[{"id":8,"name":"WeakAuras"}]`))
			} else {
				w.Write([]byte(`[{"id":1}]`))
			}
			return
		}
		if r.Header.Get("Authorization") != "main-test" {
			t.Error("wrong main-client scheme")
		}
		switch r.URL.Path {
		case "/system/user/getInfo":
			w.Write([]byte(`{"data":{"user":{"userId":7}}}`))
		case "/system/user/import/detail":
			json.NewEncoder(w).Encode(map[string]any{"data": []any{current}})
		case "/system/user/import/update":
			writes++
			var body map[string]any
			d := json.NewDecoder(r.Body)
			d.UseNumber()
			if err := d.Decode(&body); err != nil {
				t.Error(err)
			}
			if body["title"] != "Keep" || body["imageUrl"] != current["imageUrl"] {
				t.Error("omitted fields changed")
			}
			if _, ok := body["requiredTierId"]; ok {
				t.Error("null did not clear tier")
			}
			if body["contentText"] != "New text" {
				t.Errorf("HTML text projection %v", body["contentText"])
			}
			current = body
			w.Write([]byte(`{"code":200,"data":true}`))
		default:
			t.Errorf("unexpected route %s", r.URL.Path)
			w.WriteHeader(404)
		}
	}))
	defer srv.Close()
	c := &modusClient{http: NewHTTP(), origin: srv.URL + "/", static: srv.URL + "/", token: "main-test", main: true}
	s, _ := ModusInput("wa edit")
	input, err := s.Decode([]byte(`{"server_type":0,"import_id":"wa-one","content":"<p>New&nbsp;text</p><script>hidden</script>","required_tier_id":null}`))
	if err != nil {
		t.Fatal(err)
	}
	result, err := c.run(context.Background(), "wa edit", Request{Home: t.TempDir(), RequestID: "wa-edit-one"}, input)
	if err != nil || !result.Complete || writes != 1 {
		t.Fatalf("main edit: %+v %v writes=%d", result, err, writes)
	}
}
func TestModusMainReadExplicitFalseZeroAndSchemas(t *testing.T) {
	var body map[string]any
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			t.Error(err)
		}
		if r.Header.Get("X-Server-Type") != "0" {
			t.Error("Build zero lost")
		}
		w.Write([]byte(`{"data":{"rows":[],"total":0}}`))
	}))
	defer srv.Close()
	c := &modusClient{http: NewHTTP(), origin: srv.URL + "/", token: "test", main: true}
	s, _ := ModusInput("wa list")
	in, err := s.Decode([]byte(`{"server_type":0,"mine":false,"status":0,"platform":0,"is_paid":0}`))
	if err != nil {
		t.Fatal(err)
	}
	if _, err = c.readMain(context.Background(), "wa list", in); err != nil {
		t.Fatal(err)
	}
	if body["mine"] != false || body["status"] != float64(0) || body["isPaid"] != float64(0) {
		t.Fatalf("false/zero changed: %v", body)
	}
	for _, action := range ModusActions() {
		spec, ok := ModusInput(action)
		if !ok {
			t.Fatalf("no contract %s", action)
		}
		raw, _ := json.Marshal(spec.Example)
		if _, err = spec.Decode(raw); err != nil {
			t.Fatalf("invalid example %s: %v", action, err)
		}
	}
	r, err := RunModus(context.Background(), "options builds", Request{Home: t.TempDir()})
	if err != nil || !r.Complete {
		t.Fatalf("offline catalog: %+v %v", r, err)
	}
}

func TestModusWAVersionReadbackIncludesRequestedChangelog(t *testing.T) {
	for _, requested := range []string{"New release notes", ""} {
		t.Run(requested, func(t *testing.T) {
			writes := 0
			actual := "Old release notes"
			srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				switch r.URL.Path {
				case "/system/user/getInfo":
					w.Write([]byte(`{"data":{"userId":7}}`))
				case "/system/user/import/detail":
					json.NewEncoder(w).Encode(map[string]any{"data": []any{map[string]any{"id": "wa-one", "versions": []any{map[string]any{"version": "2", "codeText": "original-code", "changelog": actual}}}}})
				case "/system/user/import/version/publish":
					writes++
					var body map[string]any
					if err := json.NewDecoder(r.Body).Decode(&body); err != nil || body["changelog"] != requested {
						t.Errorf("submitted changelog = %v, err=%v", body, err)
					}
					w.Write([]byte(`{"code":200,"data":true}`))
				default:
					t.Errorf("unexpected route %s", r.URL.Path)
				}
			}))
			defer srv.Close()
			c := &modusClient{http: NewHTTP(), origin: srv.URL + "/", main: true}
			raw, _ := json.Marshal(map[string]any{"server_type": 0, "import_id": "wa-one", "version": "2", "code_text": "original-code", "changelog": requested})
			spec, _ := ModusInput("wa version-publish")
			input, err := spec.Decode(raw)
			if err != nil {
				t.Fatal(err)
			}
			r := Request{Home: t.TempDir(), RequestID: "version-notes"}
			result, err := c.run(context.Background(), "wa version-publish", r, input)
			if err == nil || result.Complete || writes != 1 {
				t.Fatalf("stale notes accepted: complete=%v writes=%d err=%v", result.Complete, writes, err)
			}
			actual = requested
			result, err = c.run(context.Background(), "operation verify", r, map[string]any{"request": r.RequestID})
			if err != nil || !result.Complete || writes != 1 {
				t.Fatalf("matching readback: complete=%v writes=%d err=%v", result.Complete, writes, err)
			}
		})
	}
}

func TestModusWAVersionDeleteRequiresSelectedImportMembership(t *testing.T) {
	for _, belongs := range []bool{false, true} {
		t.Run(map[bool]string{false: "foreign-version", true: "selected-version"}[belongs], func(t *testing.T) {
			writes := 0
			versions := []any{map[string]any{"versionId": "original-version"}}
			if belongs {
				versions = append(versions, map[string]any{"versionId": "delete-version"})
			}
			srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				switch r.URL.Path {
				case "/system/user/getInfo":
					w.Write([]byte(`{"data":{"userId":7}}`))
				case "/system/user/import/detail":
					json.NewEncoder(w).Encode(map[string]any{"data": []any{map[string]any{"id": "selected-wa", "versions": versions}}})
				case "/system/user/import/version/delete":
					writes++
					if r.Method != "DELETE" || r.URL.Query().Get("versionId") != "delete-version" {
						t.Errorf("unexpected deletion: %s %s", r.Method, r.URL)
					}
					versions = []any{map[string]any{"versionId": "original-version"}}
					w.Write([]byte(`{"code":200,"data":true}`))
				default:
					t.Errorf("unexpected route %s", r.URL.Path)
				}
			}))
			defer srv.Close()
			c := &modusClient{http: NewHTTP(), origin: srv.URL + "/", main: true}
			spec, _ := ModusInput("wa version-delete")
			input, err := spec.Decode([]byte(`{"server_type":0,"import_id":"selected-wa","version_id":"delete-version","confirm":"DELETE"}`))
			if err != nil {
				t.Fatal(err)
			}
			r := Request{Home: t.TempDir(), RequestID: "delete-version"}
			result, err := c.run(context.Background(), "wa version-delete", r, input)
			if !belongs {
				if err == nil || result.Complete || writes != 0 {
					t.Fatalf("foreign version accepted: complete=%v writes=%d err=%v", result.Complete, writes, err)
				}
				return
			}
			if err != nil || !result.Complete || writes != 1 {
				t.Fatalf("selected version deletion: complete=%v writes=%d err=%v", result.Complete, writes, err)
			}
			result, err = c.run(context.Background(), "operation resume", r, map[string]any{"request": r.RequestID})
			if err != nil || !result.Complete || writes != 1 {
				t.Fatalf("completed deletion replayed: complete=%v writes=%d err=%v", result.Complete, writes, err)
			}
		})
	}
}

func TestModusMainPlanRejectsInvalidPlatformSelection(t *testing.T) {
	for _, action := range []string{"config edit", "wa edit"} {
		for _, change := range []string{`"platform":2`, `"synchronization_type":2`, `"platform":3,"required_tier_id":5`, `"synchronization_type":3,"required_tier_id":5`} {
			key := "share_id"
			if action == "wa edit" {
				key = "import_id"
			}
			input := []byte(`{"server_type":0,"` + key + `":"selected",` + change + `}`)
			result, err := RunModus(context.Background(), action, Request{Plan: true, Input: input})
			if err == nil || result.Complete {
				t.Errorf("plan accepted %s %s: %+v %v", action, change, result, err)
			}
		}
	}
}
