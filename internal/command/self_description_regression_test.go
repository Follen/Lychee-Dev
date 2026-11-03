package command

import (
	"io/fs"
	"os"
	"path/filepath"
	"reflect"
	"strconv"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/records"
)

func TestSchemaNavigationBoundsExecute(t *testing.T) {
	root, pin := dataCommandFixture(t)
	for _, tc := range []struct {
		route   string
		args    []string
		flag    string
		maximum int
	}{
		{"data db2 search", []string{"Map", "--field", "Name_lang", "--query", "dusk"}, "--limit", records.MaxSearchRows},
		{"data db2 foreign-key", []string{"Map", "--field", "ParentMapID", "--value", "1"}, "--limit", records.MaxForeignKeyRows},
		{"data decor list", nil, "--limit", records.MaxDecorListRows},
		{"data spell info", []string{"--spell-id", "100"}, "--max-depth", records.MaxSpellDepth},
		{"data encounter get", []string{"--journal-encounter-id", "600"}, "--max-depth", records.MaxEncounterDepth},
	} {
		t.Run(tc.route, func(t *testing.T) {
			words := strings.Fields(tc.route)
			schemaArgs := append([]string{"schema"}, words...)
			schemaResult, code := invoke(t, append(schemaArgs, "--format=json")...)
			if code != 0 {
				t.Fatalf("schema failed: %+v", schemaResult)
			}
			var maximum float64
			var defaultValue any
			for _, input := range schemaResult.Result.(map[string]any)["inputs"].([]any) {
				field := input.(map[string]any)
				if field["name"] == tc.flag {
					maximum = field["maximum"].(float64)
					defaultValue = field["default"]
				}
			}
			if maximum != float64(tc.maximum) {
				t.Errorf("schema maximum=%v, domain maximum=%d", maximum, tc.maximum)
			}
			args := append(append([]string{}, words...), tc.args...)
			args = append(args, dataArgs(pin, root, "--format=json")...)
			omitted, omittedCode := invoke(t, args...)
			if tc.flag == "--limit" {
				if defaultValue != nil || omittedCode != 2 || omitted.OK {
					t.Errorf("mandatory limit must have no default and reject omission: default=%v code=%d", defaultValue, omittedCode)
				}
			} else {
				wantDefault := records.DefaultSpellDepth
				if tc.route == "data encounter get" {
					wantDefault = records.MaxEncounterDepth
				}
				if defaultValue != float64(wantDefault) {
					t.Errorf("depth default=%v, want %d", defaultValue, wantDefault)
				}
				if omittedCode != 0 || !omitted.OK {
					t.Fatalf("omitted depth failed: %d %+v", omittedCode, omitted)
				}
				for _, depth := range []int{0, wantDefault} {
					explicit, code := invoke(t, append(append([]string{}, args...), tc.flag, strconv.Itoa(depth))...)
					if code != 0 || !explicit.OK {
						t.Fatalf("explicit depth %d failed: code=%d", depth, code)
					}
					for _, field := range []string{"chainDepth", "spells", "triggers", "descRefs", "sections", "relatedSpells", "complete", "truncated", "partial"} {
						if !reflect.DeepEqual(explicit.Result.(map[string]any)[field], omitted.Result.(map[string]any)[field]) {
							t.Errorf("explicit depth %d differs from omitted default in %s", depth, field)
						}
					}
				}
			}
			result, code := invoke(t, append(append([]string{}, args...), tc.flag, strconv.FormatFloat(maximum, 'f', 0, 64))...)
			if code != 0 || !result.OK {
				t.Errorf("advertised maximum rejected: code=%d result=%+v", code, result)
			}
			result, code = invoke(t, append(append([]string{}, args...), tc.flag, strconv.Itoa(tc.maximum+1))...)
			if code != 2 || result.OK {
				t.Errorf("domain maximum+1 accepted: code=%d result=%+v", code, result)
			}
		})
	}
}

func TestHotfixExportRejectsUnsupportedEncodingBeforeWrites(t *testing.T) {
	root, _, file, _ := hotfixCommandFixture(t)
	files := func() map[string]string {
		t.Helper()
		result := map[string]string{}
		if err := filepath.WalkDir(root, func(path string, entry fs.DirEntry, err error) error {
			if err != nil {
				return err
			}
			if entry.IsDir() {
				return nil
			}
			data, err := os.ReadFile(path)
			if err == nil {
				result[path] = string(data)
			}
			return err
		}); err != nil {
			t.Fatal(err)
		}
		return result
	}
	for _, encoding := range []string{"png", "raw", "webp", "", "csv"} {
		t.Run("encoding="+encoding, func(t *testing.T) {
			output := filepath.Join(t.TempDir(), "hotfix.export")
			args := raidbotsArgs(file, "--home", root, "--format=json")
			if encoding != "csv" {
				args = append(args, "--output", output)
			}
			if encoding != "" {
				args = append(args, "--encoding", encoding)
			}
			before := files()
			result, code := invoke(t, args...)
			if code != 2 || result.OK {
				t.Errorf("unsupported export returned success/non-argument error: code=%d result=%+v", code, result)
			}
			if _, err := os.Stat(output); !os.IsNotExist(err) {
				t.Errorf("unexpected output: %v", err)
			}
			if after := files(); !reflect.DeepEqual(before, after) {
				t.Error("invalid export wrote workspace state")
			}
		})
	}
	output := filepath.Join(t.TempDir(), "hotfix.csv")
	result, code := invoke(t, raidbotsArgs(file, "--encoding", "csv", "--output", output, "--home", root, "--format=json")...)
	if code != 0 || !result.OK {
		t.Fatalf("valid CSV export: code=%d result=%+v", code, result)
	}
	if data, err := os.ReadFile(output); err != nil || len(data) == 0 {
		t.Fatalf("CSV was not written: %v", err)
	}
	if _, err := os.Stat(output + ".manifest.json"); err != nil {
		t.Fatal(err)
	}
}
