package command

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// raidbotsArgs queries the synthetic DBCache fixture through the raidbots
// snapshot source; the query context is declared, never inferred.
func raidbotsArgs(file string, extra ...string) []string {
	return append([]string{"data", "hotfix", "--source", "raidbots", "--raidbots", file,
		"--product", "retail", "--build", "12.1.0.69875", "--region", "us", "--locale", "enUS"}, extra...)
}

func TestDataHotfixRaidbotsSourceAndCSV(t *testing.T) {
	root, _, file, _ := hotfixCommandFixture(t)
	missingFile, code := invoke(t, "data", "hotfix", "--source", "raidbots", "--product", "retail", "--build", "12.1.0.69875", "--region", "us", "--locale", "enUS", "--home", root, "--format=json")
	if code != 2 || missingFile.OK || !strings.Contains(missingFile.Error.Message, "--raidbots") {
		t.Fatalf("missing raidbots file: %d %+v", code, missingFile)
	}
	missingContext, code := invoke(t, "data", "hotfix", "--source", "raidbots", "--raidbots", file, "--home", root, "--format=json")
	if code != 2 || missingContext.OK || !strings.Contains(missingContext.Error.Message, "--product") {
		t.Fatalf("missing query context: %d %+v", code, missingContext)
	}

	result, code := invoke(t, append(raidbotsArgs(file), "--home", root, "--format=json")...)
	if code != 0 || !result.OK {
		t.Fatalf("raidbots query: %d %+v", code, result)
	}
	unified := result.Result.(map[string]any)
	records := unified["records"].([]any)
	if len(records) != 3 {
		t.Fatalf("raidbots records: %+v", unified)
	}
	coverage := unified["coverage"].(map[string]any)
	if coverage["provider"] != "raidbots" || coverage["complete"] != false || coverage["returned"] != float64(3) {
		t.Fatalf("raidbots coverage: %+v", coverage)
	}
	snapshot := unified["snapshot"].(string)
	if snapshot == "" || result.Context["snapshot"] != snapshot {
		t.Fatalf("raidbots did not pin the change: %+v", result.Context)
	}

	filtered, code := invoke(t, append(raidbotsArgs(file, "--search", "x"), "--home", root, "--format=json")...)
	if code != 2 || filtered.Error.Code != "records.hotfix_filter_unsupported" || filtered.Result != nil {
		t.Fatalf("search filter: %d %+v", code, filtered)
	}

	// The typed JSONL stream carries one record frame per hotfix record.
	var out, log bytes.Buffer
	if code := Execute(context.Background(), append(raidbotsArgs(file, "--home", root, "--format=jsonl"), "--limit", "2"), &out, &log); code != 0 {
		t.Fatalf("jsonl stream: %d %s", code, log.String())
	}
	frames := []map[string]any{}
	decoder := json.NewDecoder(&out)
	for {
		var frame map[string]any
		if err := decoder.Decode(&frame); err != nil {
			if errors.Is(err, io.EOF) {
				break
			}
			t.Fatalf("decode: %v: %s", err, out.String())
		}
		frames = append(frames, frame)
	}
	if len(frames) != 4 {
		t.Fatalf("jsonl frames: %+v", frames)
	}
	if frames[0]["type"] != "begin" || frames[3]["type"] != "end" {
		t.Fatalf("frame order: %+v", frames)
	}
	end := frames[3]
	// The window stopped at the explicit limit before the source was exhausted.
	if end["count"] != float64(2) || end["complete"] != false || end["truncated"] != true {
		t.Fatalf("end frame: %+v", end)
	}

	output := filepath.Join(t.TempDir(), "hotfix.csv")
	manifest, code := invoke(t, append(raidbotsArgs(file, "--home", root, "--format=json", "--encoding", "csv", "--output", output), "--limit", "2")...)
	if code != 0 || !manifest.OK {
		t.Fatalf("csv export: %d %+v", code, manifest)
	}
	exported := manifest.Result.(map[string]any)
	if exported["schema"] != "lycheedev.csv-export.v1" || exported["rowCount"] != float64(2) || len(exported["columns"].([]any)) != 9 {
		t.Fatalf("csv manifest: %+v", exported)
	}
	raw, err := os.ReadFile(output)
	if err != nil || !strings.HasPrefix(string(raw), "id,push_id,record_id,table_name,status,build,region_id,locale,payload_length\r\n") {
		t.Fatalf("csv bytes: %v %q", err, raw)
	}
	if lines := strings.Split(strings.TrimRight(string(raw), "\r\n"), "\r\n"); len(lines) != 3 {
		t.Fatalf("csv rows: %q", raw)
	}
	var sidecar map[string]any
	sidecarRaw, err := os.ReadFile(output + ".manifest.json")
	if err != nil || json.Unmarshal(sidecarRaw, &sidecar) != nil || sidecar["rowCount"] != float64(2) {
		t.Fatalf("csv sidecar manifest: %v %s", err, sidecarRaw)
	}

	// CSV stays a remote-result encoding; the pinned local-cache flow refuses it.
	cached, code := invoke(t, "data", "hotfix", "--source", "dbcache", "--snapshot", "PIN-x", "--dbcache", file, "--encoding", "csv", "--output", output, "--home", root, "--format=json")
	if code != 2 || cached.OK || !strings.Contains(cached.Error.Message, "dbcache") {
		t.Fatalf("dbcache csv: %d %+v", code, cached)
	}
}

func TestDataHotfixWagoSourceOffline(t *testing.T) {
	root, _, _, _ := hotfixCommandFixture(t)
	// No page receipt is cached in this workspace, so offline mode must refuse
	// the network instead of silently returning nothing.
	offline, code := invoke(t, "data", "hotfix", "--source", "wago", "--product", "retail", "--build", "12.0.7.68974", "--region", "us", "--locale", "enUS", "--offline", "--limit", "5", "--home", root, "--format=json")
	if code != 3 || offline.Error.Code != "records.hotfix_offline" || offline.Result != nil {
		t.Fatalf("wago offline: %d %+v", code, offline)
	}
	badTime, code := invoke(t, "data", "hotfix", "--source", "wago", "--product", "retail", "--build", "12.0.7.68974", "--region", "us", "--locale", "enUS", "--from", "yesterday", "--home", root, "--format=json")
	if code != 2 || badTime.OK {
		t.Fatalf("wago bad timestamp: %d %+v", code, badTime)
	}
}

func TestDataTableVerbArgumentRouting(t *testing.T) {
	// The three-word paths must route to their own verbs while the two-word
	// row read keeps working with --table.
	root, pin := dataCommandFixture(t)
	wrong, code := invoke(t, append([]string{"data", "db2", "--table", "Map", "Extra"}, dataArgs(pin, root, "--format=json")...)...)
	if code != 2 || wrong.OK || wrong.Error == nil || wrong.Error.Code != "command.invalid_arguments" {
		t.Fatalf("unexpected argument: %d %+v", code, wrong)
	}
	unknown, code := invoke(t, append([]string{"data", "db2", "unknown"}, dataArgs(pin, root, "--format=json")...)...)
	if code != 2 || unknown.OK || unknown.Error == nil || !strings.Contains(unknown.Error.Message, "unexpected argument") {
		t.Fatalf("unknown db2 verb: %d %+v", code, unknown)
	}
}
