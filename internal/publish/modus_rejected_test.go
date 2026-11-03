package publish

import (
	"context"
	"errors"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"testing"
)

func requireRejectedExit(t *testing.T, result Result, err error) {
	t.Helper()
	var pe *Error
	if !errors.As(err, &pe) || pe.Exit != 5 || result.Complete || result.Submission != "rejected" {
		t.Errorf("rejected result must retain business failure: %+v, %v", result, err)
	}
	if data, ok := result.Data.(map[string]any); ok && String(data, "nextCommand") != "" {
		t.Errorf("rejected result suggests recovery: %+v", data)
	}
}

func TestModusRejectedVersionCannotVerifyExistingValue(t *testing.T) {
	for _, tc := range []struct {
		name     string
		status   int
		body     string
		rejected bool
	}{
		{"business", 200, `{"code":403,"success":false}`, true},
		{"unauthorized", 401, `{}`, true},
		{"forbidden", 403, `{}`, true},
		{"server-error", 503, `{}`, false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			writes := 0
			srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				switch r.URL.Path {
				case "/system/user/getInfo":
					w.Write([]byte(`{"data":{"userId":7}}`))
				case "/system/user/import/detail":
					w.Write([]byte(`{"data":[{"id":"wa-one","versions":[{"version":"2","codeText":"same"}]}]}`))
				case "/system/user/import/version/publish":
					writes++
					w.WriteHeader(tc.status)
					w.Write([]byte(tc.body))
				default:
					t.Errorf("unexpected route %s", r.URL.Path)
				}
			}))
			defer srv.Close()
			c := &modusClient{http: NewHTTP(), origin: srv.URL + "/", main: true}
			spec, _ := ModusInput("wa version-publish")
			in, err := spec.Decode([]byte(`{"server_type":0,"import_id":"wa-one","version":"2","code_text":"same"}`))
			if err != nil {
				t.Fatal(err)
			}
			req := Request{Home: t.TempDir(), RequestID: "rejected-version"}
			first, err := c.run(context.Background(), "wa version-publish", req, in)
			if err == nil {
				t.Fatal("rejection hidden")
			}
			if tc.rejected {
				requireRejectedExit(t, first, err)
			}
			for _, action := range []string{"operation verify", "operation resume"} {
				result, err := c.run(context.Background(), action, req, map[string]any{"request": req.RequestID})
				if result.Complete == tc.rejected || (err == nil) == tc.rejected || writes != 1 {
					t.Fatalf("%s: complete=%t writes=%d err=%v", action, result.Complete, writes, err)
				}
				if tc.rejected {
					requireRejectedExit(t, result, err)
				}
			}
			if tc.rejected {
				result, err := c.run(context.Background(), "wa version-publish", req, in)
				requireRejectedExit(t, result, err)
				status, err := RunModus(context.Background(), "operation status", Request{Home: req.Home, Input: []byte(`{"request":"rejected-version"}`)})
				if err != nil || status.Submission != "rejected" || writes != 1 {
					t.Fatalf("status: %+v %v, writes=%d", status, err, writes)
				}
			}
		})
	}
}

func TestModusMediaHTTPRejectionRetainsSubmission(t *testing.T) {
	for _, status := range []int{401, 403, 503} {
		t.Run(http.StatusText(status), func(t *testing.T) {
			writes := 0
			srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				writes++
				w.WriteHeader(status)
			}))
			defer srv.Close()
			home := t.TempDir()
			file := filepath.Join(home, "image.png")
			if err := os.WriteFile(file, []byte("original image"), 0600); err != nil {
				t.Fatal(err)
			}
			in := map[string]any{"file": file}
			op := modusOperation{Schema: "lycheedev.publish.modus-operation.v1", ID: "media-response", Action: "media upload", Account: "7", Source: "main", Input: in, InputDigest: modusInputDigest(in), Stage: "prepare", Submission: "not_submitted"}
			c := &modusClient{http: NewHTTP(), origin: srv.URL + "/", main: true}
			if err := c.continueMedia(context.Background(), home, &op); err == nil {
				t.Fatal("HTTP error hidden")
			}
			saved, err := readModusOperation(home, op.ID)
			want := "rejected"
			if status == 503 {
				want = "uncertain"
			}
			if err != nil || saved.Submission != want || writes != 1 {
				t.Fatalf("submission=%s want=%s writes=%d err=%v", saved.Submission, want, writes, err)
			}
		})
	}
}

func TestModusBackupRejectedRenameRetainsOriginalOutcome(t *testing.T) {
	writes := 0
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/system/user/getInfo":
			w.Write([]byte(`{"data":{"userId":7}}`))
		case "/system/user/backup/detail/1":
			w.Write([]byte(`{"data":{"id":1,"serverType":0,"backupName":"same"}}`))
		case "/system/user/backup/update":
			writes++
			w.Write([]byte(`{"code":403,"success":false}`))
		default:
			t.Errorf("unexpected route %s", r.URL.Path)
		}
	}))
	defer srv.Close()
	c := &modusClient{http: NewHTTP(), origin: srv.URL + "/", main: true}
	req := Request{Home: t.TempDir(), RequestID: "backup-rejected"}
	in := modusBackupTestInput(t, "backup rename", map[string]any{"server_type": 0, "backup_id": 1, "backup_name": "same"})
	first, err := runModusBackup(context.Background(), "backup rename", req, in, c)
	requireRejectedExit(t, first, err)
	for _, action := range []string{"backup verify", "backup resume"} {
		result, err := runModusBackup(context.Background(), action, req, map[string]any{"request": req.RequestID}, c)
		if err == nil || result.Complete || result.Submission != "rejected" || writes != 1 {
			t.Fatalf("%s: %+v writes=%d err=%v", action, result, writes, err)
		}
		requireRejectedExit(t, result, err)
	}
	result, err := runModusBackup(context.Background(), "backup rename", req, in, c)
	requireRejectedExit(t, result, err)
	status, err := RunModus(context.Background(), "backup status", Request{Home: req.Home, Input: []byte(`{"request":"backup-rejected"}`)})
	if err != nil || status.Submission != "rejected" || writes != 1 {
		t.Fatalf("status: %+v %v, writes=%d", status, err, writes)
	}
}
