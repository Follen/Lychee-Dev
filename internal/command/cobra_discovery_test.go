package command

import (
	"bytes"
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"reflect"
	"strconv"
	"strings"
	"testing"
)

func parseOptions(args []string) (Options, error) { return parseCommand(context.Background(), args) }

func TestCobraLiteralArgumentsBooleansAndIsolation(t *testing.T) {
	first, err := parseOptions([]string{"--format=json", "--snapshot=PIN-x", "source", "query", "--", "--format=text"})
	if err != nil || first.format != "json" || first.words[2] != "--format=text" {
		t.Fatalf("%+v %v", first, err)
	}
	second, err := parseOptions([]string{"data", "db2", "--cdn=false", "--installation", "a path with spaces", "--id=0"})
	if err != nil || second.cdn || second.installation != "a path with spaces" || !second.recordIDSet || second.recordID != 0 || second.snapshot != "" || second.format != "text" {
		t.Fatalf("%+v %v", second, err)
	}
	for _, args := range [][]string{{"data", "sql", "--offline=true", "--offline=false"}, {"data", "sql", "--offline=maybe"}, {"source", "query", "--", "one", "two"}, {"source", "query", "x", "--unknown"}} {
		if _, err := parseOptions(args); err == nil {
			t.Fatalf("invalid args accepted: %v", args)
		}
	}
	old, err := parseOptions([]string{"data", "db2", "schema", "SpellName", "--cdn"})
	if err != nil {
		t.Fatal(err)
	}
	alias, err := parseOptions([]string{"data", "table", "schema", "SpellName", "--cdn"})
	if err != nil || !reflect.DeepEqual(old, alias) {
		t.Fatalf("alias changed behavior: %+v %+v %v", old, alias, err)
	}
}

func TestDiscoveryIsOfflineAndSchemasUseActualGrammar(t *testing.T) {
	dir := t.TempDir()
	t.Chdir(dir)
	for _, args := range [][]string{{"discover", "无限服", "法术查询"}, {"schema", "data", "sql"}, {"schema", "data", "table", "schema"}} {
		var out, stderr bytes.Buffer
		code := Execute(context.Background(), append(args, "--format=json"), &out, &stderr)
		var value Envelope
		if code != 0 || json.Unmarshal(out.Bytes(), &value) != nil || !value.OK || stderr.Len() != 0 {
			t.Fatalf("%v: %d %s %s", args, code, out.String(), stderr.String())
		}
	}
	entries, err := os.ReadDir(dir)
	if err != nil || len(entries) != 0 {
		t.Fatalf("discovery wrote workspace: %v %v", entries, err)
	}
	for _, c := range commandContracts {
		schema := c.schema()
		if len(schema.Inputs) != len(c.acceptedFlagDisplays()) {
			t.Fatalf("missing inputs for %s", c.Path)
		}
		for _, example := range schema.Examples {
			if _, err := parseOptions(example); err != nil {
				t.Fatalf("%s example %v: %v", c.Path, example, err)
			}
		}
		for _, f := range schema.Inputs {
			if f.Type != "integer" {
				continue
			}
			if f.Minimum == nil || f.Maximum == nil {
				t.Fatalf("missing bounds for %s %s", c.Path, f.Name)
			}
			for _, n := range []int64{*f.Minimum, *f.Maximum} {
				opts := Options{}
				if err := bindOption(&opts, c.Path, f.Name, strconv.FormatInt(n, 10), false); err != nil {
					t.Fatalf("schema boundary rejected: %s %s=%d: %v", c.Path, f.Name, n, err)
				}
			}
		}
	}
}

func TestCobraParseErrorsRespectMachineEncoding(t *testing.T) {
	for _, format := range []string{"json", "jsonl"} {
		var out, stderr bytes.Buffer
		code := Execute(context.Background(), []string{"source", "query", "x", "--bogus", "--format", format}, &out, &stderr)
		if code != 2 || stderr.Len() != 0 || !strings.Contains(out.String(), "lycheedev.result.v1") || strings.Contains(out.String(), "Usage:") {
			t.Fatalf("%s: %d %s %s", format, code, out.String(), stderr.String())
		}
	}
}

func TestCobraOutputFormatDoesNotParseOtherFlagValues(t *testing.T) {
	for _, value := range []string{"--format=xml", "--format=json", "--format"} {
		opts, err := parseOptions([]string{"data", "sql", "--sql", value})
		if err != nil || opts.sql != value || opts.format != "text" {
			t.Fatalf("flag value %q reinterpreted: sql=%q format=%q err=%v", value, opts.sql, opts.format, err)
		}
	}
	opts, err := parseOptions([]string{"data", "sql", "--sql", "--format=xml", "--unknown", "--format=json"})
	if err == nil || opts.format != "json" {
		t.Fatalf("parse failure encoding: %+v %v", opts, err)
	}
}

func TestMissingWorkspaceReturnsExecutableInitialization(t *testing.T) {
	for _, args := range [][]string{
		{"source", "sync", "--source", "wow-ui-source", "--product", "retail", "--ref", strings.Repeat("a", 40)},
		{"target", "list"},
		{"config", "show"},
	} {
		t.Run(strings.Join(args[:2], "-"), func(t *testing.T) {
			home := filepath.Join(t.TempDir(), "new workspace")
			response, code := invoke(t, append(args, "--home", home, "--format=json")...)
			if code != 3 || response.Error == nil || response.Error.Code != "vault.workspace_missing" {
				t.Fatalf("missing workspace: code=%d response=%+v", code, response)
			}
			if _, err := os.Stat(home); !os.IsNotExist(err) {
				t.Fatalf("diagnosis changed the workspace: %v", err)
			}
			details, ok := response.Error.Details.(map[string]any)
			if !ok {
				t.Fatalf("missing recovery: %#v", response.Error.Details)
			}
			initArgs := stringList(t, details["arguments"])
			if len(initArgs) != 3 || initArgs[0] != "init" || initArgs[1] != "--home" || initArgs[2] != home {
				t.Fatalf("recovery lost workspace selection: %v", initArgs)
			}
			initialized, exit := invoke(t, append(initArgs, "--format=json")...)
			if exit != 0 || !initialized.OK {
				t.Fatalf("returned recovery failed: %d %+v", exit, initialized)
			}
			listed, exit := invoke(t, "target", "list", "--home", home, "--format=json")
			if exit != 0 || !listed.OK {
				t.Fatalf("initialized workspace unusable: %d %+v", exit, listed)
			}
		})
	}
}

func TestDiscoveryFindsProductSelectionAndPlatformRecovery(t *testing.T) {
	for _, question := range []string{"forever", "正式无限服", "国服无限服"} {
		result := discoverCommands(question)
		commands := result["commands"].([]Definition)
		found := false
		for _, command := range commands {
			found = found || command.Path == "target resolve"
		}
		if !found {
			t.Fatalf("%q did not find target resolution: %+v", question, commands)
		}
		matched, ok := result["matchedProducts"].([]map[string]any)
		if !ok || len(matched) != 1 || matched[0]["product"] != "forever" {
			t.Fatalf("%q lost product alias: %#v", question, result["matchedProducts"])
		}
	}
	for _, question := range []string{"网易DD发布提交后没收到回复，查看和核验原操作再恢复", "DD recover original operation"} {
		result := discoverCommands(question)
		found := map[string]bool{}
		for _, command := range result["commands"].([]Definition) {
			found[command.Path] = true
			if strings.HasPrefix(command.Path, "publish ") && !strings.HasPrefix(command.Path, "publish dd ") {
				t.Fatalf("%q selected another platform: %s", question, command.Path)
			}
		}
		for _, path := range []string{"publish dd operation status", "publish dd operation verify", "publish dd operation resume"} {
			if !found[path] {
				t.Fatalf("%q omitted %s: %#v", question, path, found)
			}
		}
	}
}
