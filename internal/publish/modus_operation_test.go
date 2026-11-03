package publish

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"net/http/httptest"
	"net/url"
	"os"
	"strings"
	"sync/atomic"
	"testing"
)

type modusTestTransport func(*http.Request) (*http.Response, error)

func (f modusTestTransport) RoundTrip(r *http.Request) (*http.Response, error) { return f(r) }
func TestModusReleaseStagesOriginalArtifactAndSecretIsolation(t *testing.T) {
	a := modusTestZIP(t, map[string]string{"A/A.toc": "## Interface: 120105\n"})
	raw, err := os.ReadFile(a.file.Name())
	if err != nil {
		t.Fatal(err)
	}
	var wire map[string]any
	var writes atomic.Int32
	var uploads atomic.Int32
	var account atomic.Int32
	account.Store(7)
	srv := httptest.NewTLSServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path == "/object" || strings.HasPrefix(r.URL.Path, "/modus/") {
			if r.Header.Get("Authorization") != "" || r.Header.Get("X-Device-Id") != "" {
				t.Error("account credential reached object store")
			}
			if r.Method == "PUT" {
				uploads.Add(1)
				b, _ := io.ReadAll(r.Body)
				if string(b) != string(raw) {
					t.Error("upload bytes changed")
				}
				w.WriteHeader(204)
			} else {
				w.Write(raw)
			}
			return
		}
		if r.Header.Get("Authorization") != "Bearer creator-fixture" {
			t.Error("missing Creator auth")
		}
		switch r.URL.Path {
		case "/system/user/getInfo":
			json.NewEncoder(w).Encode(map[string]any{"data": map[string]any{"user": map[string]any{"userId": account.Load()}}})
		case "/game/data/config/detail":
			w.Write([]byte(`{"data":[{"key":"wow_builds","value":"{\"wow_retail\":{\"versions\":[\"12.1.5\",\"12.2.0\"]}}"}]}`))
		case "/game/data/author/project/fileId/200000001":
			writes.Add(1)
			w.Write([]byte(`{"data":{"fileId":9}}`))
		case "/game/data/author/project/upload":
			writes.Add(1)
			d := json.NewDecoder(r.Body)
			d.UseNumber()
			if err := d.Decode(&wire); err != nil {
				t.Error(err)
			}
			if _, ok := wire["fileId"]; ok {
				t.Error("create metadata includes reserved fileId")
			}
			w.Write([]byte(`{"code":200,"data":true}`))
		case "/game/data/author/project/file/upload/signature/200000001/9":
			w.Write([]byte(`{"data":{"signedUrl":"https://storage.example/object?secret=fixture"}}`))
		case "/game/data/author/project/file/detail/9":
			out := map[string]any{}
			for k, v := range wire {
				out[k] = v
			}
			out["fileId"] = 9
			json.NewEncoder(w).Encode(map[string]any{"data": out})
		default:
			t.Errorf("unexpected route %s", r.URL.Path)
			w.WriteHeader(404)
		}
	}))
	defer srv.Close()
	httpClient := srv.Client()
	transport := httpClient.Transport
	target, _ := url.Parse(srv.URL)
	httpClient.Transport = modusTestTransport(func(r *http.Request) (*http.Response, error) {
		copy := r.Clone(r.Context())
		u := *copy.URL
		copy.URL = &u
		if u.Host == "storage.example" || u.Host == "cdn.modus.cool" {
			copy.URL.Scheme = target.Scheme
			copy.URL.Host = target.Host
		}
		return transport.RoundTrip(copy)
	})
	c := &modusClient{http: &HTTP{client: httpClient}, origin: srv.URL + "/", token: "creator-fixture"}
	input := map[string]any{"project_id": json.Number("200000001"), "file": a.file.Name(), "version": "1", "type": "release"}
	r := Request{Home: t.TempDir(), RequestID: "release-one"}
	result, err := c.run(context.Background(), "plugin upload", r, input)
	if err != nil || !result.Complete {
		t.Fatalf("upload incomplete: %+v %v", result, err)
	}
	if writes.Load() != 2 || uploads.Load() != 1 {
		t.Fatalf("stage counts %d %d", writes.Load(), uploads.Load())
	}
	if repeated, err := c.run(context.Background(), "plugin upload", r, input); err != nil || !repeated.Complete || writes.Load() != 2 || uploads.Load() != 1 {
		t.Errorf("verified original was not retained without replay: %+v %v", repeated, err)
	}
	account.Store(8)
	if _, err := c.run(context.Background(), "plugin upload", r, input); err == nil || writes.Load() != 2 || uploads.Load() != 1 {
		t.Fatal("verified request accepted a different account or replayed")
	}
	account.Store(7)
	c.main = true
	if _, err := c.run(context.Background(), "plugin upload", r, input); err == nil || writes.Load() != 2 || uploads.Load() != 1 {
		t.Fatal("verified request accepted a different login source or replayed")
	}
	c.main = false
	changed := map[string]any{}
	for key, value := range input {
		changed[key] = value
	}
	changed["version"] = "2"
	if _, err := c.run(context.Background(), "plugin upload", r, changed); err == nil || writes.Load() != 2 || uploads.Load() != 1 {
		t.Fatal("verified request accepted changed input or replayed")
	}
	saved, err := readModusOperation(r.Home, r.RequestID)
	if err != nil {
		t.Fatal(err)
	}
	b, _ := json.Marshal(saved)
	if strings.Contains(string(b), "secret=fixture") || strings.Contains(string(b), "creator-fixture") {
		t.Fatal("secret persisted")
	}
}
func TestModusUncertainProjectEditRequiresReadbackAndOriginalAccount(t *testing.T) {
	var writes atomic.Int32
	account := 7
	summary := "before"
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/system/user/getInfo":
			json.NewEncoder(w).Encode(map[string]any{"data": map[string]any{"user": map[string]any{"userId": account}}})
		case "/game/data/author/project/detail/200000001":
			json.NewEncoder(w).Encode(map[string]any{"data": map[string]any{"projectId": 200000001, "summary": summary, "synchronizationType": 1}})
		case "/game/data/author/project/update":
			writes.Add(1)
			summary = "after"
			w.WriteHeader(503)
		default:
			t.Errorf("unexpected route %s", r.URL.Path)
		}
	}))
	defer srv.Close()
	c := &modusClient{http: NewHTTP(), origin: srv.URL + "/", token: "fixture"}
	ctx := context.Background()
	r := Request{Home: t.TempDir(), RequestID: "edit-one"}
	input := map[string]any{"project_id": json.Number("200000001"), "summary": "after"}
	if _, err := c.run(ctx, "project edit", r, input); err == nil {
		t.Fatal("HTTP uncertainty was hidden")
	}
	op, err := readModusOperation(r.Home, r.RequestID)
	if err != nil || op.Submission != "uncertain" {
		t.Fatalf("intent not retained: %+v %v", op, err)
	}
	recovery := map[string]any{"request": r.RequestID}
	if _, err = c.run(ctx, "operation resume", r, recovery); err == nil || writes.Load() != 1 {
		t.Fatal("unknown write replayed")
	}
	account = 8
	if _, err = c.run(ctx, "operation verify", r, recovery); err == nil {
		t.Fatal("foreign account verified original write")
	}
	account = 7
	result, err := c.run(ctx, "operation verify", r, recovery)
	if err != nil || !result.Complete || writes.Load() != 1 {
		t.Fatalf("readback failed: %+v %v", result, err)
	}
}
func TestModusProjectFormOmissionAndNull(t *testing.T) {
	s, _ := ModusInput("project edit")
	input, err := s.Decode([]byte(`{"project_id":200000001,"alt_name":null,"description":""}`))
	if err != nil {
		t.Fatal(err)
	}
	w, err := modusProjectWire(input, false)
	if err != nil {
		t.Fatal(err)
	}
	if w["altName"] != "<null>" || w["description"] != "<null>" {
		t.Fatalf("clear semantics %v", w)
	}
	if _, ok := w["license"]; ok {
		t.Fatal("omission overwrote license")
	}
	s, _ = ModusInput("project create")
	input, err = s.Decode([]byte(`{"game":"wow","name":"A","summary":"A","categories":[998],"license":{"type":"MIT"},"publish_platforms":["modus"]}`))
	if err != nil {
		t.Fatal(err)
	}
	if _, err = modusProjectWire(input, true); err == nil {
		t.Fatal("BigFoot-only category admitted")
	}
	_, err = RunModus(context.Background(), "project edit", Request{Plan: true, Input: []byte(`{"project_id":200000001,"summary":"ok","unexpected":true}`)})
	var e *Error
	if !errors.As(err, &e) || e.Path != "$.unexpected" {
		t.Fatalf("field error lost: %v", err)
	}
}
