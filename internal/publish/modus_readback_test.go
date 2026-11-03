package publish

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

func TestModusUnsubmittedOperationCannotVerifyExistingResource(t *testing.T) {
	for _, stage := range []string{"prepare", "write", "reserve", "metadata"} {
		t.Run(stage, func(t *testing.T) {
			calls := 0
			srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				calls++
				w.Write([]byte(`{"code":200,"data":{"projectId":200000001}}`))
			}))
			defer srv.Close()
			c := &modusClient{http: NewHTTP(), origin: srv.URL + "/"}
			op := modusOperation{Action: "project create", RemoteID: "200000001", Stage: stage, Submission: "not_submitted", Input: map[string]any{"name": "never submitted"}}
			if stage == "reserve" {
				op.Action = "plugin upload"
				op.Submission = "uncertain"
			}
			err := c.verifyOperation(context.Background(), &op)
			if err == nil || op.Verified || calls != 0 {
				t.Fatalf("unsubmitted operation verified=%v calls=%d err=%v", op.Verified, calls, err)
			}
		})
	}
}

func TestModusReadAndPlanKeepRawWAAndImagesOutOfOutput(t *testing.T) {
	value := map[string]any{"content": "public article", "codeText": "!WA:2!private", "imagesOps": []any{map[string]any{"op": "upload", "base64": "private-image"}}, "token": "private-token"}
	raw, err := json.Marshal(modusSafe(value))
	if err != nil || strings.Contains(string(raw), "private") || !strings.Contains(string(raw), "public article") {
		t.Fatalf("read projection: %s %v", raw, err)
	}
	input := []byte(`{"project_id":200000001,"images":1,"image_ops":[{"op":"upload","name":"1.webp","base64":"private-image"}]}`)
	r, err := RunModus(context.Background(), "project edit", Request{Input: input, Plan: true})
	raw, _ = json.Marshal(r)
	if err != nil || !r.Complete || strings.Contains(string(raw), "private-image") || !strings.Contains(string(raw), "sha256") {
		t.Fatalf("plan projection: %s %v", raw, err)
	}
}

func TestModusDeletionRequiresCompleteDistinctResourceList(t *testing.T) {
	for _, tc := range []struct {
		name     string
		rows     []map[string]any
		total    int
		complete bool
	}{
		{"empty", nil, 0, true},
		{"other_file", []map[string]any{{"projectId": 200000001, "fileId": 8}}, 1, true},
		{"target_file", []map[string]any{{"projectId": 200000001, "fileId": 9}}, 1, false},
		{"short_page", []map[string]any{{"fileId": 8}}, 2, false},
		{"duplicate", []map[string]any{{"fileId": 8}, {"fileId": 8}}, 2, false},
		{"missing_id", []map[string]any{{"projectId": 200000001}}, 1, false},
		{"wrong_total", []map[string]any{{"fileId": 8}}, 0, false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				if r.URL.Path != "/game/data/author/project/file/list" {
					t.Errorf("unexpected route %s", r.URL.Path)
				}
				rows := tc.rows
				if rows == nil {
					rows = []map[string]any{}
				}
				json.NewEncoder(w).Encode(map[string]any{"code": 200, "data": map[string]any{"rows": rows, "total": tc.total}})
			}))
			defer srv.Close()
			c := &modusClient{http: NewHTTP(), origin: srv.URL + "/"}
			op := modusOperation{Action: "plugin delete", RemoteID: "9", Stage: "readback", Input: map[string]any{"project_id": json.Number("200000001")}}
			err := c.verifyOperation(context.Background(), &op)
			if (err == nil) != tc.complete || op.Verified != tc.complete {
				t.Fatalf("verified=%v err=%v", op.Verified, err)
			}
		})
	}
}

func TestModusMetadataOnlyReleaseRejectsUnavailableVersionsBeforeWrite(t *testing.T) {
	writes := 0
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path == "/game/data/config/detail" {
			w.Write([]byte(`{"code":200,"data":[{"key":"wow_builds","value":"{\"wow_retail\":{\"versions\":[\"12.1.5\"]}}"}]}`))
			return
		}
		writes++
		t.Errorf("unexpected call %s", r.URL.Path)
	}))
	defer srv.Close()
	c := &modusClient{http: NewHTTP(), origin: srv.URL + "/"}
	op := modusOperation{Action: "plugin edit", Stage: "prepare", Input: map[string]any{
		"project_id": json.Number("200000001"), "file_id": json.Number("9"),
		"supported_game_versions": []any{map[string]any{"gameVersion": "99.0.0", "server": "wow_retail"}},
	}}
	if err := c.continueRelease(context.Background(), t.TempDir(), &op); err == nil || writes != 0 {
		t.Fatalf("err=%v writes=%d", err, writes)
	}
}
