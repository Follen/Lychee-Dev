package publish

import (
	"context"
	"encoding/json"
	"net/url"
	"testing"
)

func TestDDPluginEditRequiresCompleteAuthorVersion(t *testing.T) {
	for _, missing := range []string{"version", "release_type", "file_path"} {
		t.Run(missing, func(t *testing.T) {
			latest := map[string]any{"version": "new", "release_type": "release", "file_path": "https://example.com/new.zip"}
			delete(latest, missing)
			api := &ddFake{fn: func(string, url.Values, any, bool) (map[string]any, error) {
				return ddEnvelope([]any{map[string]any{"sn": "p1", "latest_version": latest}}), nil
			}}
			current := map[string]any{"game_type": json.Number("10001"), "version": "stale", "release_type": "alpha", "detail_url": "https://example.com/old.zip"}
			if _, err := ddProjectPlugin(context.Background(), api, current, "p1", true); err == nil {
				t.Fatal("incomplete author projection reused stale detail fields")
			}
		})
	}
}

func TestDDPluginModifyPreservesTopLevelEditorFields(t *testing.T) {
	current := map[string]any{"game_type": json.Number("10001"), "game_versions": []any{"current-game"}, "update_desc": "current editor notes", "latest_version": map[string]any{"version": "v1", "release_type": "release", "file_path": "https://example.com/current.zip", "game_versions": []any{"old-game"}, "update_desc": "old version notes"}}
	api := &ddFake{fn: func(string, url.Values, any, bool) (map[string]any, error) {
		return ddEnvelope([]any{map[string]any{"sn": "p1", "latest_version": current["latest_version"]}}), nil
	}}
	for _, authorRequired := range []bool{false, true} {
		form, err := ddProjectPlugin(context.Background(), api, current, "p1", authorRequired)
		if err != nil || !ddEqual(form["game_versions"], []any{"current-game"}) || form["update_desc"] != "current editor notes" {
			t.Fatalf("modify replaced preserved top-level fields: %#v %v", form, err)
		}
	}
}

func TestDDPluginReadbackUsesIndependentOfficialProjections(t *testing.T) {
	for _, scenario := range []string{"detail-version", "author-logo", "mixed-version", "neither-logo", "editor-notes"} {
		t.Run(scenario, func(t *testing.T) {
			detail := map[string]any{"sn": "p1", "game_type": json.Number("10001"), "logo": "https://example.com/new.png", "latest_version": map[string]any{"version": "new", "release_type": "release", "file_path": "https://example.com/new.zip"}}
			author := ddClone(detail)
			expected := map[string]any{"version": "new", "release_type": "release", "detail_url": "https://example.com/new.zip", "logo": "https://example.com/new.png"}
			switch scenario {
			case "detail-version":
				author["latest_version"] = map[string]any{"version": "old", "release_type": "alpha", "file_path": "https://example.com/old.zip"}
			case "author-logo":
				detail["logo"] = "https://example.com/old.png"
			case "mixed-version":
				ddMap(detail["latest_version"])["file_path"] = "https://example.com/old.zip"
				ddMap(author["latest_version"])["version"] = "old"
			case "neither-logo":
				detail["logo"], author["logo"] = "https://example.com/old.png", "https://example.com/old.png"
			case "editor-notes":
				expected["update_desc"] = "current editor notes"
				detail["update_desc"], author["update_desc"] = "current editor notes", "current editor notes"
				ddMap(detail["latest_version"])["update_desc"] = "old version notes"
				ddMap(author["latest_version"])["update_desc"] = "old version notes"
			}
			api := &ddFake{fn: func(path string, q url.Values, body any, mutation bool) (map[string]any, error) {
				if mutation {
					t.Fatal("readback replayed mutation")
				}
				if path == "/addon/detail_v2" {
					return ddEnvelope(detail), nil
				}
				if path == "/addon/addon_list" {
					return ddEnvelope([]any{author}), nil
				}
				t.Fatalf("unexpected route %s", path)
				return nil, nil
			}}
			r := ddReceipt{Schema: "lycheedev.publish.dd-operation.v1", Request: "plugin-readback", Author: "author-1", Action: "plugin edit", SN: "p1", Game: 10001, Expected: expected, Submission: "acknowledged", Verification: "pending"}
			if scenario == "detail-version" || scenario == "mixed-version" {
				r.Action = "plugin update"
			}
			got, err := ddVerify(context.Background(), api, t.TempDir(), r, "operation verify")
			want := scenario == "detail-version" || scenario == "author-logo" || scenario == "editor-notes"
			if got.Complete != want || (err == nil) != want {
				t.Fatalf("complete=%v err=%v want=%v", got.Complete, err, want)
			}
		})
	}
}
