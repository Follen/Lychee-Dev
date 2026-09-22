package command

import (
	"context"
	"encoding/json"
	"github.com/follenfang/lycheedev/internal/records"
	"github.com/follenfang/lycheedev/internal/selection"
	"os"
	"path/filepath"
	"strconv"
	"testing"
)

func TestAssetInspectionArguments(t *testing.T) {
	for _, args := range [][]string{
		{"asset", "inspect"},
		{"asset", "inspect", "--snapshot", "x", "--installation", "x"},
		{"asset", "inspect", "--file-id", "0"},
		{"asset", "inspect", "--file-id", "4294967296"},
		{"asset", "inspect", "--file-id", "-1"},
		{"asset", "inspect", "--max-bytes", "536870913"},
		{"asset", "inspect", "--max-bytes", "0"},
		{"version", "--installation", "x"},
		{"version", "--file-id", "1"},
		{"source", "inspect", "--max-bytes", "1"},
	} {
		result, code := invoke(t, append(args, "--format=json")...)
		if code != 2 || result.OK {
			t.Fatalf("%v: %+v (%d)", args, result, code)
		}
	}
}

// Opt-in real command routing, immutable pin, shared vault and evidence check.
// It reads no account data, sends no input, and never modifies the game install.
func TestInstalledAssetCommand(t *testing.T) {
	installation := os.Getenv("LYCHEEDEV_TEST_INSTALLATION")
	if installation == "" {
		t.Skip("explicit installed build fixture required")
	}
	if os.Getenv("LYCHEEDEV_TEST_PRODUCT") != "wow" || os.Getenv("LYCHEEDEV_TEST_BUILD") != "12.1.0.69875" {
		t.Fatal("fixture requires wow 12.1.0.69875")
	}
	meta, err := records.ResolveLocalBuild(context.Background(), installation, "wow", "12.1.0.69875")
	if err != nil {
		t.Fatal(err)
	}
	root := filepath.Join(t.TempDir(), "workspace")
	if result, code := invoke(t, "init", "--home", root, "--format=json"); code != 0 {
		t.Fatalf("%+v", result)
	}
	spec := selection.SelectionSpec{Data: &selection.DataPin{Product: "retail", Region: "us", Language: "enUS", FullBuild: meta.Installed.FullBuild, BuildConfig: meta.Installed.BuildConfig, CDNConfig: meta.Installed.CDNConfig, DefinitionCommit: "83057bdc0cbe13062850ebf8ad530031e128a1cd"}}
	raw, err := json.Marshal(spec)
	if err != nil {
		t.Fatal(err)
	}
	input := filepath.Join(t.TempDir(), "selection.json")
	if err := os.WriteFile(input, raw, 0600); err != nil {
		t.Fatal(err)
	}
	result, code := invoke(t, "target", "resolve", "--file", input, "--home", root, "--format=json")
	if code != 0 {
		t.Fatalf("pin: %+v", result)
	}
	pin := result.Result.(map[string]any)["id"].(string)
	result, code = invoke(t, "asset", "inspect", "--snapshot", pin, "--installation", installation, "--file-id", "1361031", "--home", root, "--format=json")
	if code != 0 || !result.OK || len(result.Captures) != 1 {
		t.Fatalf("asset: %+v (%d)", result, code)
	}
	content := result.Result.(map[string]any)["content"].(map[string]any)
	if content["sha256"] != "4ffc543ad33964d34fdb8f645b90152a5c94831923a8c30e4fd5972000f2fe20" || content["bytes"] != float64(6452) {
		t.Fatalf("wrong content: %+v", content)
	}
	capture := result.Captures[0].(map[string]any)
	verified, code := invoke(t, "evidence", "verify", capture["id"].(string), "--home", root, "--format=json")
	if code != 0 || !verified.OK {
		t.Fatalf("capture: %+v", verified)
	}
	t.Logf("pin=%s capture=%s content=%v", pin, capture["id"], content)
	limited, code := invoke(t, "asset", "inspect", "--snapshot", pin, "--installation", installation, "--file-id", "1361031", "--max-bytes", "1", "--home", root, "--format=json")
	if code == 0 || limited.OK || len(limited.Captures) != 0 || limited.Result != nil {
		t.Fatalf("size failure exposed success or partial output: %+v (%d)", limited, code)
	}
	if os.Getenv("LYCHEEDEV_TEST_DBD_NETWORK") == "1" {
		args := []string{"data", "db2", "--snapshot", pin, "--installation", installation, "--table", "ChrClasses", "--id", "1", "--home", root, "--format=json"}
		missing, code := invoke(t, append(append([]string{}, args...), "--offline")...)
		if code == 0 || missing.OK || missing.Result != nil {
			t.Fatalf("offline cache miss hidden: %+v", missing)
		}
		query, err := records.QueryData(context.Background(), root, pin, records.FileQuery{Installation: installation, ContentBytes: 128 << 20, MetadataBytes: 512 << 20}, records.DataQuery{SQL: "SELECT ID, Filename FROM ChrClasses WHERE ID=:id", Parameters: map[string]any{"id": 1}})
		if err != nil {
			t.Fatal(err)
		}
		if len(query.Result.Result.Rows) != 1 || query.Result.Result.Rows[0][1] != "WARRIOR" || len(query.Result.Sources) != 1 {
			t.Fatal(query)
		}
		queryVerified, queryCode := invoke(t, "evidence", "verify", query.Capture.ID, "--home", root, "--format=json")
		if queryCode != 0 || !queryVerified.OK {
			t.Fatal(queryVerified)
		}
		t.Logf("SQL application capture=%s rows=%v", query.Capture.ID, query.Result.Result.Rows)
		queryFile := filepath.Join(t.TempDir(), "query.json")
		if err := os.WriteFile(queryFile, []byte(`{"sql":"SELECT ID, Filename FROM ChrClasses WHERE ID=:id","parameters":{"id":1}}`), 0600); err != nil {
			t.Fatal(err)
		}
		queried, queryCode := invoke(t, "data", "sql", "--file", queryFile, "--snapshot", pin, "--installation", installation, "--offline", "--home", root, "--format=json")
		if queryCode != 0 || !queried.OK || len(queried.Captures) != 1 {
			t.Fatal(queried, queryCode)
		}
		queryResult := queried.Result.(map[string]any)["result"].(map[string]any)
		queryRows := queryResult["rows"].([]any)
		if len(queryRows) != 1 || queryRows[0].([]any)[1] != "WARRIOR" {
			t.Fatal(queryResult)
		}
		queryCapture := queried.Captures[0].(map[string]any)["id"].(string)
		if verified, code := invoke(t, "evidence", "verify", queryCapture, "--home", root, "--format=json"); code != 0 {
			t.Fatal(verified)
		}
		t.Logf("SQL CLI offline capture=%s rows=%v", queryCapture, queryRows)
		for _, offline := range []bool{false, true} {
			call := append([]string{}, args...)
			if offline {
				call = append(call, "--offline")
			}
			typed, code := invoke(t, call...)
			if code != 0 || !typed.OK || len(typed.Captures) != 1 {
				t.Fatalf("typed offline=%v: %+v (%d)", offline, typed, code)
			}
			reading := typed.Result.(map[string]any)
			row := reading["row"].(map[string]any)
			if row["ID"] != float64(1) || row["Filename"] != "WARRIOR" || len(row) != 43 || reading["layoutHash"] != "AFC9B0C2" {
				t.Fatalf("wrong typed row: %+v", reading)
			}
			captureID := typed.Captures[0].(map[string]any)["id"].(string)
			if result, code := invoke(t, "evidence", "verify", captureID, "--home", root, "--format=json"); code != 0 {
				t.Fatalf("typed capture: %+v", result)
			}
			t.Logf("typed command offline=%v row=%v layout=%v capture=%s", offline, row["Filename"], reading["layoutHash"], captureID)
		}
		pageArgs := []string{"data", "db2", "--snapshot", pin, "--installation", installation, "--table", "ChrClasses", "--home", root, "--format=json", "--offline"}
		all, code := invoke(t, append(append([]string{}, pageArgs...), "--limit", "200")...)
		if code != 0 {
			t.Fatalf("full page: %+v", all)
		}
		whole := all.Result.(map[string]any)["page"].(map[string]any)
		if whole["more"] != false || all.Captures[0].(map[string]any)["complete"] != true {
			t.Fatalf("unexpected fixture full-page coverage: %+v", all)
		}
		var combined []any
		var combinedRows []any
		var cursor *uint32
		for calls := 0; calls < 20; calls++ {
			call := append(append([]string{}, pageArgs...), "--limit", "3")
			if cursor != nil {
				call = append(call, "--after-id", strconv.FormatUint(uint64(*cursor), 10))
			}
			response, code := invoke(t, call...)
			if code != 0 {
				t.Fatalf("page: %+v", response)
			}
			page := response.Result.(map[string]any)["page"].(map[string]any)
			combined = append(combined, page["ids"].([]any)...)
			combinedRows = append(combinedRows, page["rows"].([]any)...)
			capture := response.Captures[0].(map[string]any)
			if capture["complete"] != false || len(response.Warnings) == 0 {
				t.Fatalf("partial coverage hidden: %+v", response)
			}
			if page["more"] == false {
				break
			}
			value := uint32(page["next"].(float64))
			if cursor != nil && value <= *cursor {
				t.Fatal("cursor did not advance")
			}
			cursor = &value
		}
		actual, _ := json.Marshal(combined)
		expected, _ := json.Marshal(whole["ids"])
		if string(actual) != string(expected) {
			t.Fatalf("paged IDs %s != full IDs %s", actual, expected)
		}
		actualRows, _ := json.Marshal(combinedRows)
		expectedRows, _ := json.Marshal(whole["rows"])
		if string(actualRows) != string(expectedRows) {
			t.Fatal("paged typed fields differ from whole small-table result")
		}
		t.Logf("typed table page IDs equal complete small-table result: %s", actual)
	}
}

func TestDataRecordArguments(t *testing.T) {
	for _, args := range [][]string{
		{"data", "db2"},
		{"data", "db2", "--id", "-1"},
		{"data", "db2", "--id", "4294967296"},
		{"data", "db2", "--offline", "--offline"},
		{"data", "db2", "--file-id", "1"},
		{"data", "db2", "--id", "1", "--limit", "2"},
		{"data", "db2", "--id", "1", "--after-id", "0"},
		{"data", "db2", "--after-id", "-1"},
		{"data", "db2", "--after-id", "4294967296"},
		{"version", "--offline"},
		{"asset", "inspect", "--table", "Map"},
		{"asset", "inspect", "--id", "1"},
	} {
		result, code := invoke(t, append(args, "--format=json")...)
		if code != 2 || result.OK {
			t.Fatalf("%v: %+v (%d)", args, result, code)
		}
	}
	options, err := parseOptions([]string{"data", "db2", "--id", "0", "--table", "Map", "--snapshot", "pin", "--installation", "root", "--offline"})
	if err != nil || !options.recordIDSet || options.recordID != 0 || !options.offline {
		t.Fatalf("zero ID is legal: %+v %v", options, err)
	}
}
