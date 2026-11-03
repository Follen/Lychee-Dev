package publish

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"net/http"
	"os"
	"path/filepath"
	"testing"
)

func TestNewBeeCreatorRecoveryRejectsChangedDesktopSession(t *testing.T) {
	for _, action := range []string{"config edit", "config edit verify", "backup delete"} {
		t.Run(action, func(t *testing.T) {
			calls := 0
			c := newBeeFixture(t, func(w http.ResponseWriter, r *http.Request) { calls++; newBeeReply(w, []any{}) })
			c.access = "different-desktop-session"
			originalAction := "config edit"
			input := map[string]any{"id": json.Number("7"), "intro": "updated"}
			if action == "backup delete" {
				originalAction = action
				input = map[string]any{"ids": []any{json.Number("7")}, "confirm_delete": true}
			}
			fingerprint, err := newBeeFingerprint(context.Background(), "creator "+originalAction, input)
			if err != nil {
				t.Fatal(err)
			}
			home := t.TempDir()
			key := sha256.Sum256([]byte("original"))
			token := sha256.Sum256([]byte("original-desktop-session"))
			receipt := map[string]any{"fingerprint": fingerprint, "session": hex.EncodeToString(token[:]), "state": "complete", "completed": []int{7}, "result": Result{Complete: true}}
			raw, _ := json.Marshal(receipt)
			file := filepath.Join(home, "publish", "newbee", hex.EncodeToString(key[:])+".json")
			if err := os.MkdirAll(filepath.Dir(file), 0700); err != nil {
				t.Fatal(err)
			}
			if err := os.WriteFile(file, raw, 0600); err != nil {
				t.Fatal(err)
			}
			request := Request{Home: home, RequestID: "original"}
			var result Result
			switch action {
			case "config edit":
				result, err = c.run(context.Background(), action, newBeeCreatorCommands[action], input, request)
			case "config edit verify":
				result, err = c.verifyOriginal(context.Background(), originalAction, input, request)
			default:
				result, err = c.runBackupDelete(context.Background(), input, request)
			}
			if err == nil || result.Complete || calls != 0 {
				t.Fatalf("changed session recovered original operation: result=%+v err=%v network=%d", result, err, calls)
			}
		})
	}
}

func TestNewBeeCreatorDeletePlanRejectsFalseConfirmation(t *testing.T) {
	for _, resource := range []string{"plugin", "config", "wa"} {
		result, err := RunNewBee(context.Background(), "creator "+resource+" delete", Request{Plan: true, RequestID: "delete", Input: []byte(`{"id":7,"confirm_delete":false}`)})
		if err == nil || result.Complete {
			t.Errorf("%s plan accepted invalid deletion: %+v %v", resource, result, err)
		}
	}
}
func TestNewBeeBackupRenameHasOriginalVerification(t *testing.T) {
	const action = "creator backup rename verify"
	found := false
	for _, candidate := range NewBeeActions() {
		found = found || candidate == action
	}
	spec, ok := NewBeeInput(action)
	if !found || !ok {
		t.Fatal("backup rename recovery is not publicly reachable")
	}
	input, err := spec.Decode([]byte(`{"id":7,"name":"renamed"}`))
	if err != nil {
		t.Fatal(err)
	}
	name, writes := "old", 0
	client := newBeeFixture(t, func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/share/update":
			writes++
			name = "renamed"
			http.Error(w, "lost response", 500)
		case "/v3/share/list":
			newBeeReply(w, []any{map[string]any{"t_id": 7, "t_name": name}})
		default:
			t.Errorf("unexpected endpoint %s", r.URL.Path)
		}
	})
	request := Request{Home: t.TempDir(), RequestID: "rename"}
	if _, err = client.run(context.Background(), "backup rename", newBeeCreatorCommands["backup rename"], input, request); err == nil {
		t.Fatal("expected lost response")
	}
	result, err := client.verifyOriginal(context.Background(), "backup rename", input, request)
	if err != nil || !result.Complete || writes != 1 {
		t.Fatalf("rename recovery: %+v %v writes=%d", result, err, writes)
	}
}
