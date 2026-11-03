package command

import (
	"bytes"
	"context"
	"encoding/json"
	"os"
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
