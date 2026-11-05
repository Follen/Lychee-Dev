package publish

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
)

func TestReceiptStatusRejectsTrailingJSON(t *testing.T) {
	ctx := context.Background()
	for _, suffix := range []string{" \n", " {}", " garbage"} {
		t.Run(suffix, func(t *testing.T) {
			home := t.TempDir()
			if err := ddSave(ctx, home, ddReceipt{Schema: "lycheedev.publish.dd-operation.v1", Request: "original", Verification: "verified"}); err != nil {
				t.Fatal(err)
			}
			path, _ := ddReceiptPath(home, "original")
			raw, err := os.ReadFile(path)
			if err != nil {
				t.Fatal(err)
			}
			if err = os.WriteFile(path, append(raw, []byte(suffix)...), 0600); err != nil {
				t.Fatal(err)
			}
			_, err = RunDD(ctx, "operation status", Request{Home: home, Input: []byte(`{"request":"original"}`)})
			if (err == nil) != (suffix == " \n") {
				t.Errorf("DD suffix=%q err=%v", suffix, err)
			}

			digest := sha256.Sum256([]byte("original"))
			path = filepath.Join(home, "publish", "newbee", hex.EncodeToString(digest[:])+".json")
			if err = os.MkdirAll(filepath.Dir(path), 0700); err != nil {
				t.Fatal(err)
			}
			raw, _ = json.Marshal(map[string]any{"fingerprint": hex.EncodeToString(digest[:]), "state": "complete", "complete": true})
			if err = os.WriteFile(path, append(raw, []byte(suffix)...), 0600); err != nil {
				t.Fatal(err)
			}
			_, err = RunNewBee(ctx, "operation status", Request{Home: home, RequestID: "original", Input: []byte(`{}`)})
			if (err == nil) != (suffix == " \n") {
				t.Errorf("NewBee suffix=%q err=%v", suffix, err)
			}
			if err = saveCFReceipt(ctx, home, cfReceipt{Schema: "lycheedev.publish.curseforge-upload.v1", ProjectID: 1, Request: "original"}); err != nil {
				t.Fatal(err)
			}
			path, _ = cfReceiptPath(home, 1, "original")
			raw, err = os.ReadFile(path)
			if err != nil {
				t.Fatal(err)
			}
			if err = os.WriteFile(path, append(raw, []byte(suffix)...), 0600); err != nil {
				t.Fatal(err)
			}
			_, err = RunCurseForge(ctx, "plugin status", Request{Home: home, Input: []byte(`{"project_id":1,"request":"original"}`)})
			if (err == nil) != (suffix == " \n") {
				t.Errorf("CurseForge suffix=%q err=%v", suffix, err)
			}
		})
	}
}

func TestNewBeeStatusRejectsInvalidReceiptShape(t *testing.T) {
	home := t.TempDir()
	digest := sha256.Sum256([]byte("original"))
	path := filepath.Join(home, "publish", "newbee", hex.EncodeToString(digest[:])+".json")
	if err := os.MkdirAll(filepath.Dir(path), 0700); err != nil {
		t.Fatal(err)
	}
	for _, raw := range []string{"null", `{}`, `{"fingerprint":42}`, `{"fingerprint":"invalid"}`} {
		if err := os.WriteFile(path, []byte(raw), 0600); err != nil {
			t.Fatal(err)
		}
		_, err := RunNewBee(context.Background(), "operation status", Request{Home: home, RequestID: "original", Input: []byte(`{}`)})
		if err == nil {
			t.Errorf("invalid receipt reported as successfully loaded: %s", raw)
		}
	}
}
