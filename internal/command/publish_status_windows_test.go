//go:build windows

package command

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
	"time"
	"unsafe"

	"golang.org/x/sys/windows"
)

func writePublishStatusFixture(t *testing.T, path string, record any, encrypted bool) []byte {
	t.Helper()
	raw, err := json.Marshal(record)
	if err != nil {
		t.Fatal(err)
	}
	if encrypted {
		entropy := []byte("LycheeDev.Heybox.CurrentUser.v1")
		in := windows.DataBlob{Size: uint32(len(raw)), Data: &raw[0]}
		ent := windows.DataBlob{Size: uint32(len(entropy)), Data: &entropy[0]}
		var out windows.DataBlob
		if err := windows.CryptProtectData(&in, nil, &ent, 0, nil, windows.CRYPTPROTECT_UI_FORBIDDEN, &out); err != nil {
			t.Fatal(err)
		}
		runtime.KeepAlive(raw)
		runtime.KeepAlive(entropy)
		raw = append([]byte(nil), unsafe.Slice(out.Data, int(out.Size))...)
		windows.LocalFree(windows.Handle(uintptr(unsafe.Pointer(out.Data))))
	}
	if err := os.MkdirAll(filepath.Dir(path), 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, raw, 0600); err != nil {
		t.Fatal(err)
	}
	return raw
}

func TestPublishOriginalStatusReadsDoNotBecomePending(t *testing.T) {
	for _, route := range []string{"curseforge plugin status", "dd operation status", "newbee operation status", "modus operation status", "modus backup status", "blackbox operation status"} {
		t.Run(strings.ReplaceAll(route, " ", "_"), func(t *testing.T) {
			for _, state := range []string{"rejected", "uncertain", "verified", "missing", "invalid"} {
				t.Run(state, func(t *testing.T) {
					home := t.TempDir()
					id := "original-status"
					complete := state == "verified"
					input := map[string]any{"request": id}
					digest := sha256.Sum256([]byte("{}"))
					emptyDigest := hex.EncodeToString(digest[:])
					var record map[string]any
					var path string
					encrypted := false
					expectedSubmission := state
					switch route {
					case "dd operation status":
						verification := "pending"
						if complete {
							verification = "verified"
						}
						record = map[string]any{"schema": "lycheedev.publish.dd-operation.v1", "request": id, "action": "backup rename", "author": "fixture", "submission": state, "verification": verification}
						path = filepath.Join(home, "publish", "dd", "operations", id+".json")
					case "curseforge plugin status":
						input["project_id"] = 123
						verification := "pending"
						if complete {
							verification = "remote_file_identity"
						}
						record = map[string]any{"schema": "lycheedev.publish.curseforge-upload.v1", "request": id, "projectId": 123, "submission": state, "verification": verification}
						path = filepath.Join(home, "publish", "curseforge", "uploads", "123", id+".json")
					case "modus operation status":
						record = map[string]any{"schema": "lycheedev.publish.modus-operation.v1", "id": id, "action": "config edit", "source": "main", "account": "fixture", "input": map[string]any{}, "inputDigest": emptyDigest, "stage": state, "submission": state, "verified": complete}
						path = filepath.Join(home, "publish", "modus", "operations", id+".json")
					case "modus backup status":
						phase := state
						if state == "uncertain" {
							phase = "finish-intent"
						}
						if complete {
							expectedSubmission = "submitted"
						}
						record = map[string]any{"schema": "lycheedev.publish.modus-backup.v1", "id": id, "action": "backup upload", "account": "fixture", "input": map[string]any{}, "inputDigest": emptyDigest, "phase": phase, "complete": complete}
						path = filepath.Join(home, "publish", "modus", "backups", id, "state.json")
					case "blackbox operation status":
						stepState := state
						if complete {
							stepState = "verified"
							expectedSubmission = "submitted"
						}
						record = map[string]any{"schema": "lycheedev.blackbox.operation.v1", "id": id, "action": "backup rename", "method": "sms", "account": "fixture", "input": map[string]any{}, "digest": emptyDigest, "stage": state, "steps": []any{map[string]any{"state": stepState}}, "verified": complete}
						path = filepath.Join(home, "publish", "blackbox", "operations", id+".bin")
						encrypted = true
					case "newbee operation status":
						input = map[string]any{}
						hash := sha256.Sum256([]byte(id))
						record = map[string]any{"fingerprint": emptyDigest, "state": state, "complete": complete}
						path = filepath.Join(home, "publish", "newbee", hex.EncodeToString(hash[:])+".json")
					}
					var original []byte
					if state != "missing" {
						if state == "invalid" {
							record = nil
						}
						original = writePublishStatusFixture(t, path, record, encrypted)
						if state == "invalid" {
							original = []byte("{")
							if err := os.WriteFile(path, original, 0600); err != nil {
								t.Fatal(err)
							}
						}
					}
					file := filepath.Join(home, "status-input.json")
					writePublishStatusFixture(t, file, input, false)
					args := append([]string{"publish"}, strings.Fields(route)...)
					args = append(args, "--file", file, "--home", home, "--format=json")
					if route == "newbee operation status" {
						args = append(args, "--request", id)
					}
					result, code := invoke(t, args...)
					if state == "missing" || state == "invalid" {
						if code == 0 || result.OK || result.Error == nil {
							t.Fatalf("bad receipt accepted: %d %+v", code, result)
						}
						return
					}
					if code != 0 || !result.OK || result.Error != nil {
						t.Fatalf("read original %s: %d %+v", state, code, result)
					}
					data := resultMap(t, result)
					if route == "newbee operation status" {
						originalState := data["data"].(map[string]any)
						if originalState["state"] != state || originalState["complete"] != complete {
							t.Fatal(data)
						}
					} else if data["complete"] != complete || data["submission"] != expectedSubmission {
						t.Fatalf("original state rewritten: %+v", data)
					}
					after, err := os.ReadFile(path)
					if err != nil || !bytes.Equal(original, after) {
						t.Fatalf("status modified receipt: %v", err)
					}
					if route == "dd operation status" && state == "uncertain" {
						for i := range args {
							if args[i] == "--format=json" {
								args[i] = "--format=jsonl"
							}
						}
						var out, stderr bytes.Buffer
						exit := Execute(context.Background(), args, &out, &stderr)
						lines := strings.Split(strings.TrimSpace(out.String()), "\n")
						var last map[string]any
						if err := json.Unmarshal([]byte(lines[len(lines)-1]), &last); err != nil {
							t.Fatal(err)
						}
						if exit != 0 || stderr.Len() != 0 || last["type"] != "end" {
							t.Fatalf("status JSONL: %d %s %s", exit, out.String(), stderr.String())
						}
					}
				})
			}
		})
	}
}

func TestPublishLoginStatusStillReportsPending(t *testing.T) {
	home := t.TempDir()
	id := strings.Repeat("a", 32)
	now := time.Now().Unix()
	record := map[string]any{"schema": "lycheedev.blackbox.pending-login.v1", "login_id": id, "status": "waiting_code", "Created": now, "Expires": now + 300, "session": map[string]any{"method": "sms"}}
	writePublishStatusFixture(t, filepath.Join(home, "publish", "blackbox", "pending.bin"), record, true)
	file := filepath.Join(home, "input.json")
	writePublishStatusFixture(t, file, map[string]any{"login_id": id}, false)
	response, code := invoke(t, "publish", "blackbox", "session", "login-status", "--file", file, "--home", home, "--format=json")
	if code != 6 || response.OK || response.Error == nil || response.Error.Code != "publish.pending" {
		t.Fatalf("login wait became read completion: %d %+v", code, response)
	}
}
