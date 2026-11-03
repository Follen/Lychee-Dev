package publish

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"net/url"
	"os"
	"path/filepath"
	"strconv"
	"testing"
	"time"
)

func assertDDRejectedExit(t *testing.T, err error) {
	t.Helper()
	var pe *Error
	if !errors.As(err, &pe) || pe.Code != "publish.dd_rejected" || pe.Exit != 5 {
		t.Errorf("rejected operation must retain business failure exit 5: %v", err)
	}
}

func TestDDBackupRejectedStagesCannotVerifyOrResume(t *testing.T) {
	for _, rejected := range []string{"/backup/pre_create", "/backup/finish_create", "/backup/modify_name"} {
		t.Run(rejected, func(t *testing.T) {
			ctx := context.Background()
			root := t.TempDir()
			file := filepath.Join(root, "Interface", "AddOns", "A", "A.lua")
			if err := os.MkdirAll(filepath.Dir(file), 0700); err != nil {
				t.Fatal(err)
			}
			if err := os.WriteFile(file, []byte("return 1"), 0600); err != nil {
				t.Fatal(err)
			}
			generated, err := ddBackupGenerate(ctx, map[string]any{"root": root, "output": filepath.Join(t.TempDir(), "bundle"), "game_type": json.Number("1"), "unknown_addons": []any{"A"}})
			if err != nil {
				t.Fatal(err)
			}
			file = ddText(ddMap(generated)["bundle_file"])
			opened, err := ddLoadBundle(file)
			if err != nil {
				t.Fatal(err)
			}
			remote := ddClone(opened.bundle.Manifest)
			opened.close()
			remote["sn"], remote["name"] = "created", "chosen"
			writes := 0
			api := &ddFake{fn: func(path string, _ url.Values, _ any, mutation bool) (map[string]any, error) {
				if mutation {
					writes++
				}
				if path == rejected {
					return nil, failure("publish.dd_rejected", "fixture rejected")
				}
				switch path {
				case "/game_type/list":
					return ddEnvelope([]any{map[string]any{"game_type": json.Number("1")}}), nil
				case "/backup/pre_create":
					return ddEnvelope(map[string]any{"sn": "created", "unknown_addon": map[string]any{"u_url": ""}}), nil
				case "/backup/finish_create", "/backup/modify_name":
					return ddEnvelope(nil), nil
				case "/backup/detail":
					return ddEnvelope(remote), nil
				case "/backup/list":
					return ddEnvelope([]any{map[string]any{"sn": "created"}}), nil
				}
				t.Fatalf("unexpected %s", path)
				return nil, nil
			}}
			req := Request{Home: t.TempDir(), RequestID: "rejected-stage"}
			result, err := ddBackupUpload(ctx, api, req, map[string]any{"file": file, "name": "chosen"})
			if err == nil || result.Complete {
				t.Fatal(result, err)
			}
			assertDDRejectedExit(t, err)
			saved, err := ddLoad(req.Home, req.RequestID)
			if err != nil {
				t.Fatal(err)
			}
			if saved.Submission != "rejected" {
				t.Errorf("saved submission=%s", saved.Submission)
			}
			before := writes
			result, err = ddVerifyBackup(ctx, api, req.Home, saved, "operation verify")
			if err == nil || result.Complete {
				t.Error("rejected stage verified", result, err)
			}
			assertDDRejectedExit(t, err)
			result, err = ddResumeBackup(ctx, api, req.Home, saved)
			if err == nil || result.Complete || writes != before {
				t.Error("rejected stage resumed", result, err, writes, before)
			}
			assertDDRejectedExit(t, err)
			result, err = ddBackupUpload(ctx, api, req, map[string]any{"file": file, "name": "chosen"})
			assertDDRejectedExit(t, err)
			if result.Complete || writes != before {
				t.Error("rejected upload repeated", result, writes, before)
			}
		})
	}
}

func TestDDBatchRejectedDeleteCannotVerifyLaterAbsence(t *testing.T) {
	ctx := context.Background()
	present, writes := true, 0
	api := &ddFake{fn: func(path string, _ url.Values, _ any, _ bool) (map[string]any, error) {
		switch path {
		case "/game_type/list":
			return ddEnvelope([]any{map[string]any{"game_type": json.Number("1")}}), nil
		case "/share/list":
			return ddEnvelope(map[string]any{"list": []any{}, "total": 0}), nil
		case "/backup/list":
			rows := []any{}
			if present {
				rows = append(rows, map[string]any{"sn": "b", "game_type": json.Number("1")})
			}
			return ddEnvelope(rows), nil
		case "/backup/delete":
			writes++
			return nil, failure("publish.dd_rejected", "fixture rejected")
		}
		t.Fatalf("unexpected %s", path)
		return nil, nil
	}}
	req := Request{Home: t.TempDir(), RequestID: "rejected-delete"}
	_, err := ddDeleteBackups(ctx, api, req, map[string]any{"backup_ids": []any{"b"}, "confirm_delete": true}, false, false)
	if err == nil || writes != 1 {
		t.Fatal(err, writes)
	}
	assertDDRejectedExit(t, err)
	present = false // A separate action removes the item after this request failed.
	for _, readOnly := range []bool{true, false} {
		result, err := ddDeleteBackups(ctx, api, req, map[string]any{"request": req.RequestID}, true, readOnly)
		if err == nil || result.Complete || result.Submission != "rejected" || writes != 1 {
			t.Fatal(result, err, writes)
		}
		assertDDRejectedExit(t, err)
	}
	_, err = ddDeleteBackups(ctx, api, req, map[string]any{"backup_ids": []any{"b"}, "confirm_delete": true}, false, false)
	assertDDRejectedExit(t, err)
}

type ddResponseTestSigner struct{}

func (ddResponseTestSigner) Sign(_ context.Context, target string, _ []byte, _ [][2]string) (string, error) {
	return target, nil
}
func (ddResponseTestSigner) Close() error { return nil }

func TestDDTransportDistinguishesMissingStatusFromRejection(t *testing.T) {
	for _, tc := range []struct {
		body              string
		rejected, success bool
	}{
		{`{"result":{}}`, false, false}, {`{"code":null,"result":{}}`, false, false},
		{`{"code":"bad","result":{}}`, false, false}, {`{"code":200.5,"result":{}}`, false, false},
		{`{"code":403,"result":{}}`, true, false}, {`{"code":200,"result":{}}`, false, true},
	} {
		t.Run(tc.body, func(t *testing.T) {
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) { _, _ = w.Write([]byte(tc.body)) }))
			defer server.Close()
			transport := ddTransport{http: NewHTTP(), signer: ddResponseTestSigner{}, cookie: "fixture", origin: server.URL, timestamp: 1, sampled: time.Now()}
			_, err := transport.call(context.Background(), "/backup/modify_name", nil, map[string]any{}, true)
			var pe *Error
			rejected := errors.As(err, &pe) && pe.Code == "publish.dd_rejected"
			if rejected != tc.rejected || (err == nil) != tc.success {
				t.Fatalf("error=%v rejected=%t", err, rejected)
			}
		})
	}
}

func TestDDHTTPAuthorizationRejectionCannotVerifyExistingValue(t *testing.T) {
	for _, status := range []int{http.StatusUnauthorized, http.StatusForbidden, http.StatusServiceUnavailable} {
		t.Run(strconv.Itoa(status), func(t *testing.T) {
			writes := 0
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				w.Header().Set("Content-Type", "application/json")
				switch r.URL.Path {
				case "/backup/detail":
					_, _ = w.Write([]byte(`{"code":200,"result":{"sn":"b","name":"same","game_type":1}}`))
				case "/backup/list":
					_, _ = w.Write([]byte(`{"code":200,"result":[{"sn":"b","name":"same","game_type":1}]}`))
				case "/backup/modify_name":
					writes++
					w.WriteHeader(status)
					_, _ = w.Write([]byte(`{"message":"fixture HTTP failure"}`))
				default:
					t.Errorf("unexpected route %s", r.URL.Path)
					w.WriteHeader(http.StatusNotFound)
				}
			}))
			defer server.Close()
			transport := ddTransport{http: NewHTTP(), signer: ddResponseTestSigner{}, cookie: "fixture", origin: server.URL, timestamp: 1, sampled: time.Now()}
			api := &ddFake{fn: func(path string, q url.Values, body any, mutation bool) (map[string]any, error) {
				value, err := transport.call(context.Background(), path, q, body, mutation)
				if err == nil {
					return value, nil
				}
				// Exercise the worker's JSON error boundary before mutation handling.
				reply := ddSessionReply{Error: ddSessionError(err)}
				raw, marshalErr := json.Marshal(reply)
				if marshalErr != nil || ddJSON(raw, &reply) != nil {
					t.Fatal("worker error roundtrip", marshalErr)
				}
				details, _ := reply.Error.Details.(map[string]any)
				if ddText(details["httpStatus"]) != strconv.Itoa(status) {
					t.Fatalf("worker lost HTTP status: %#v", reply.Error.Details)
				}
				return nil, reply.Error
			}}
			req := Request{Home: t.TempDir(), RequestID: "http-auth-rejection"}
			_, err := ddMutationRun(context.Background(), api, req, "backup rename", map[string]any{"sn": "b", "name": "same"})
			if err == nil || writes != 1 {
				t.Fatal("mutation result", err, writes)
			}
			if status != http.StatusServiceUnavailable {
				assertDDRejectedExit(t, err)
			} else {
				var pe *Error
				if !errors.As(err, &pe) || pe.Exit != 6 {
					t.Fatalf("unknown outcome must remain pending: %v", err)
				}
			}
			saved, err := ddLoad(req.Home, req.RequestID)
			if err != nil {
				t.Fatal(err)
			}
			rejected := status != http.StatusServiceUnavailable
			if (saved.Submission == "rejected") != rejected {
				t.Errorf("HTTP %d saved submission=%s", status, saved.Submission)
			}
			result, err := ddVerify(context.Background(), api, req.Home, saved, "operation verify")
			if result.Complete == rejected || (err == nil) == rejected || writes != 1 {
				t.Fatalf("HTTP %d verification: %+v, %v, writes=%d", status, result, err, writes)
			}
			if rejected {
				assertDDRejectedExit(t, err)
				_, err = ddMutationRun(context.Background(), api, req, "backup rename", map[string]any{"sn": "b", "name": "same"})
				assertDDRejectedExit(t, err)
				if writes != 1 {
					t.Fatal("rejected mutation replayed")
				}
				raw, _ := json.Marshal(map[string]any{"request": req.RequestID})
				statusResult, statusErr := RunDD(context.Background(), "operation status", Request{Home: req.Home, Input: raw})
				if statusErr != nil || statusResult.Submission != "rejected" || statusResult.Complete {
					t.Fatalf("status must read retained rejection: %+v, %v", statusResult, statusErr)
				}
			}
		})
	}
}
