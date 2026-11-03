//go:build windows

package publish

import (
	"context"
	"encoding/json"
	"net/http"
	"os"
	"path/filepath"
	"testing"
)

func TestNewBeeCreatorHoldsOriginalFileDuringPreparation(t *testing.T) {
	file := filepath.Join(t.TempDir(), "Addon.zip")
	if err := os.WriteFile(file, []byte("original archive"), 0600); err != nil {
		t.Fatal(err)
	}
	attempted, changed := false, false
	client := newBeeFixture(t, func(w http.ResponseWriter, r *http.Request) {
		attempted = true
		changed = os.WriteFile(file, []byte("different archive"), 0600) == nil
		http.Error(w, "stop before upload", 500)
	})
	input := map[string]any{"id": json.Number("7"), "file": file, "version": "1.0.0", "game_version_list": []any{"1.0.0"}}
	_, err := client.run(context.Background(), "plugin upload", newBeeCreatorCommands["plugin upload"], input, Request{Home: t.TempDir(), RequestID: "original"})
	if err == nil || !attempted || changed {
		t.Fatalf("original file changed between fingerprint and upload: attempted=%v changed=%v err=%v", attempted, changed, err)
	}
}
