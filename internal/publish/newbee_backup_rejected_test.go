package publish

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"testing"
)

func TestNewBeeBackupUploadRetainsRejectedStage(t *testing.T) {
	for _, stage := range []string{"prepare", "object", "create", "rename"} {
		t.Run(stage, func(t *testing.T) {
			dir := t.TempDir()
			game := filepath.Join(dir, "game")
			source := filepath.Join(game, "Interface", "AddOns", "One")
			if err := os.MkdirAll(source, 0700); err != nil {
				t.Fatal(err)
			}
			if err := os.WriteFile(filepath.Join(source, "One.toc"), []byte("toc"), 0600); err != nil {
				t.Fatal(err)
			}
			packed, err := newBeePack(context.Background(), map[string]any{"game_dir": game, "output_dir": filepath.Join(dir, "bundle"), "game_version_id": json.Number("1"), "unknown_plugins": []any{"One"}})
			if err != nil {
				t.Fatal(err)
			}
			rows := []any{}
			calls, rejected := 0, 0
			var server *httptest.Server
			server = httptest.NewTLSServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				calls++
				phase := map[string]string{"/share/generate_upload_url_v2": "prepare", "/blob": "object", "/v3/share/create": "create", "/share/update": "rename"}[r.URL.Path]
				if phase == stage {
					rejected++
					if phase == "object" {
						w.WriteHeader(http.StatusForbidden)
					} else {
						json.NewEncoder(w).Encode(map[string]any{"code": 0, "data": nil})
					}
					return
				}
				switch r.URL.Path {
				case "/metadata":
					json.NewEncoder(w).Encode(map[string]any{"game_version": []any{map[string]any{"id": 1}}})
				case "/v3/share/list":
					newBeeReply(w, rows)
				case "/share/generate_upload_url_v2":
					newBeeReply(w, map[string]any{"file_path": "object/one", "upload_url": server.URL + "/blob"})
				case "/blob":
					w.WriteHeader(http.StatusOK)
				case "/v3/share/create":
					rows = []any{map[string]any{"t_id": 11, "t_name": "existing", "t_Versionid": 1, "t_Known_plug": []any{}, "t_unKnown_list": []any{map[string]any{"name": "One"}}, "t_material_list": []any{}, "t_font_list": []any{}, "t_unKnown_plug": "object/one", "t_material": nil, "t_font_ncs_hash": nil, "wtflist": []any{}}}
					newBeeReply(w, nil)
				default:
					t.Errorf("unexpected route %s", r.URL.Path)
				}
			}))
			defer server.Close()
			c := &newBeeCreator{http: &HTTP{client: server.Client()}, api: server.URL, next: server.URL, metadata: server.URL + "/metadata", headers: http.Header{"Token": {"fixture"}}, access: "fixture"}
			input := map[string]any{"bundle_file": String(packed.(map[string]any), "bundle_file"), "name": "existing"}
			request := Request{Home: filepath.Join(dir, "home"), RequestID: "original"}
			_, err = c.runBackupUpload(context.Background(), input, request)
			if err == nil || rejected != 1 {
				t.Fatalf("stage not rejected: %v, %d", err, rejected)
			}
			before := calls
			result, err := c.runBackupUpload(context.Background(), input, request)
			if err == nil || result.Complete || result.Submission != "rejected" || calls != before {
				t.Fatalf("rejected stage %s recovered: %+v, %v, calls=%d/%d", stage, result, err, calls, before)
			}
			status, err := newBeeOperationStatus(request)
			if err != nil || status.Data.(map[string]any)["rejected"] != true {
				t.Fatalf("status omitted persisted rejection: %+v, %v", status, err)
			}
		})
	}
}

func TestNewBeeBackupDeleteRetainsRejection(t *testing.T) {
	calls, deletes := 0, 0
	c := newBeeFixture(t, func(w http.ResponseWriter, r *http.Request) {
		calls++
		switch r.URL.Path {
		case "/v3/share/list":
			newBeeReply(w, []any{map[string]any{"t_id": 7, "t_name": "existing"}})
		case "/creator/wow/share_config/publish_list":
			newBeeReply(w, map[string]any{"list": []any{}, "total": 0})
		case "/v3/share/delete":
			deletes++
			json.NewEncoder(w).Encode(map[string]any{"code": 0, "data": nil})
		default:
			t.Errorf("unexpected route %s", r.URL.Path)
		}
	})
	input := map[string]any{"ids": []any{json.Number("7")}, "confirm_delete": true}
	req := Request{Home: t.TempDir(), RequestID: "delete"}
	if _, err := c.runBackupDelete(context.Background(), input, req); err == nil || deletes != 1 {
		t.Fatalf("missing reject: %v", err)
	}
	before := calls
	result, err := c.runBackupDelete(context.Background(), input, req)
	if err == nil || result.Complete || result.Submission != "rejected" || calls != before {
		t.Fatalf("rejected delete recovered: %+v, %v, calls=%d/%d", result, err, calls, before)
	}
}
