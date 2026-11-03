package publish

import (
	"context"
	"encoding/json"
	"net/url"
	"os"
	"path/filepath"
	"testing"
)

func TestDDBackupUploadChecksCurrentGameBeforeAllocation(t *testing.T) {
	root := t.TempDir()
	file := filepath.Join(root, "Interface", "AddOns", "A", "Main.lua")
	if err := os.MkdirAll(filepath.Dir(file), 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(file, []byte("return 1"), 0600); err != nil {
		t.Fatal(err)
	}
	generated, err := ddBackupGenerate(context.Background(), map[string]any{"root": root, "output": filepath.Join(t.TempDir(), "bundle"), "game_type": json.Number("1"), "unknown_addons": []any{"A"}})
	if err != nil {
		t.Fatal(err)
	}
	file = ddText(ddMap(generated)["bundle_file"])
	writes := 0
	api := &ddFake{fn: func(path string, _ url.Values, _ any, mutation bool) (map[string]any, error) {
		if path == "/game_type/list" {
			return ddEnvelope([]any{map[string]any{"game_type": json.Number("2")}}), nil
		}
		if mutation {
			writes++
			return nil, failure("publish.dd_rejected", "game unavailable")
		}
		t.Fatalf("unexpected path %s", path)
		return nil, nil
	}}
	req := Request{Home: t.TempDir(), RequestID: "retired-game"}
	_, err = ddBackupUpload(context.Background(), api, req, map[string]any{"file": file})
	if err == nil || writes != 0 {
		t.Fatalf("unavailable game wrote %d mutations: %v", writes, err)
	}
	if _, err := ddLoad(req.Home, req.RequestID); !os.IsNotExist(err) {
		t.Fatalf("unavailable game created operation: %v", err)
	}
}
