package publish

import (
	"context"
	"encoding/json"
	"strings"
	"testing"
)

func TestCurseForgePlanExplainsVersionSelection(t *testing.T) {
	for _, selection := range []string{"unspecified", "explicit", "parent"} {
		t.Run(selection, func(t *testing.T) {
			input := map[string]any{"project_id": 123, "file": publishZip(t), "changelog": "", "release_type": "release"}
			if selection == "explicit" {
				input["game_version_names"] = []string{"12.1.5"}
			}
			if selection == "parent" {
				input["parent_file_id"] = 42
			}
			raw, _ := json.Marshal(input)
			result, err := RunCurseForge(context.Background(), "plugin upload", Request{Home: t.TempDir(), Input: raw, Plan: true})
			if err != nil || !result.Complete {
				t.Fatal(result, err)
			}
			data := ddMap(result.Data)
			if data["versionSelection"] != selection || data["completionScope"] != "local_plan" || data["remoteConditions"] == nil {
				t.Fatalf("missing preparation guidance: %#v", data)
			}
			if selection == "unspecified" && !strings.Contains(ddText(data["versionGuidance"]), "publish curseforge plugin game-versions") {
				t.Fatal(data)
			}
		})
	}
	spec, _ := CurseForgeInput("plugin upload")
	if spec.DynamicOptions["game_version_names"] != "publish curseforge plugin game-versions" {
		t.Fatal(spec.DynamicOptions)
	}
}

func assertDDRecoveryInput(t *testing.T, req Request, session string) {
	t.Helper()
	raw, _ := json.Marshal(map[string]any{"request": req.RequestID})
	result, err := RunDD(context.Background(), "operation status", Request{Home: req.Home, Input: raw})
	if err != nil {
		t.Fatal(err)
	}
	data := ddMap(result.Data)
	recovery := ddMap(data["recoveryInput"])
	if recovery["request"] != req.RequestID || recovery["session"] != session || data["replayAllowed"] != false {
		t.Fatalf("missing original recovery input: %#v", data)
	}
	for _, action := range []string{"operation verify", "operation resume"} {
		spec, _ := DDInput(action)
		raw, _ = json.Marshal(recovery)
		if _, err := spec.Decode(raw); err != nil {
			t.Fatal(action, err)
		}
	}
}

func TestDDLegacyReceiptExplainsMissingSession(t *testing.T) {
	home := t.TempDir()
	receipt := ddReceipt{Schema: "lycheedev.publish.dd-operation.v1", Request: "legacy", Author: "author-1", Action: "backup rename", Submission: "uncertain", Verification: "pending"}
	if err := ddSave(context.Background(), home, receipt); err != nil {
		t.Fatal(err)
	}
	result, err := RunDD(context.Background(), "operation status", Request{Home: home, Input: []byte(`{"request":"legacy"}`)})
	if err != nil {
		t.Fatal(err)
	}
	data := ddMap(result.Data)
	if data["recoveryInput"] != nil || !strings.Contains(ddText(data["sessionGuidance"]), "session start") {
		t.Fatal(data)
	}
}
