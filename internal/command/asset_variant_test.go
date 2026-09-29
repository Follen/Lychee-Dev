package command

import (
	"bytes"
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/testkit"
	"github.com/follenfang/lycheedev/internal/vault"
)

func TestAssetExportContentVariants(t *testing.T) {
	for _, reverse := range []bool{false, true} {
		t.Run(map[bool]string{false: "low-first", true: "standard-first"}[reverse], func(t *testing.T) {
			root := t.TempDir()
			if _, err := vault.Initialize(context.Background(), root); err != nil {
				t.Fatal(err)
			}
			standard, low := []byte("MD20 standard geometry"), []byte("MD20 alternative geometry")
			files := []testkit.FileVariant{
				{FileDataID: 189896, ContentFlags: 0x120c0080, LocaleMask: 0x1f3f6, FileObject: testkit.FileObject{Decoded: low}},
				{FileDataID: 189896, ContentFlags: 0x120c0000, LocaleMask: 0x1f3f6, FileObject: testkit.FileObject{Decoded: standard}},
			}
			if reverse {
				files[0], files[1] = files[1], files[0]
			}
			pin := testkit.CachedFileVariants(t, root, files)
			output := filepath.Join(t.TempDir(), "model.m2")
			args := []string{"asset", "export", "--snapshot", pin.ID, "--cdn", "--offline", "--file-id", "189896", "--output", output, "--home", root, "--format=json"}
			result, code := invoke(t, args...)
			if code == 0 || result.OK || result.Error.Message != "records.file_ambiguous" {
				t.Fatalf("default must preserve ambiguity: %+v %d", result, code)
			}
			if _, err := os.Stat(output); !os.IsNotExist(err) {
				t.Fatalf("published ambiguous output: %v", err)
			}
			for _, tc := range []struct {
				variant string
				body    []byte
				flags   uint32
			}{{"standard", standard, 0x120c0000}, {"low-violence", low, 0x120c0080}} {
				selected := append(append([]string{}, args...), "--content-variant", tc.variant, "--overwrite")
				result, code = invoke(t, selected...)
				if code != 0 || !result.OK {
					t.Fatalf("%s: %+v %d", tc.variant, result, code)
				}
				body, err := os.ReadFile(output)
				if err != nil || !bytes.Equal(body, tc.body) {
					t.Fatalf("wrong %s bytes: %q %v", tc.variant, body, err)
				}
				source := result.Result.(map[string]any)["source"].(map[string]any)
				if source["contentVariant"] != tc.variant || source["contentVerified"] != true || source["entry"].(map[string]any)["contentFlags"] != float64(tc.flags) {
					t.Fatal(source)
				}
			}
		})
	}
}

func TestAssetContentVariantArguments(t *testing.T) {
	for _, value := range []string{"auto", "first", "zhCN", "STANDARD", ""} {
		result, code := invoke(t, "asset", "inspect", "--content-variant", value, "--format=json")
		if code != 2 || result.OK || !strings.Contains(result.Error.Message, "content-variant") {
			t.Fatalf("%q: %+v %d", value, result, code)
		}
	}
	result, code := invoke(t, "data", "sql", "--content-variant", "standard", "--format=json")
	if code != 2 || result.OK {
		t.Fatal(result, code)
	}
}

func TestAssetContentVariantPreservesIntegrityAndExistingOutput(t *testing.T) {
	root := t.TempDir()
	if _, err := vault.Initialize(context.Background(), root); err != nil {
		t.Fatal(err)
	}
	files := []testkit.FileVariant{
		{FileDataID: 11, LocaleMask: 0x40, FileObject: testkit.FileObject{Decoded: []byte("standard bytes")}},
		{FileDataID: 11, LocaleMask: 0x40, ContentFlags: 0x80, FileObject: testkit.FileObject{Decoded: []byte("expected low bytes"), Object: append([]byte{'B', 'L', 'T', 'E', 0, 0, 0, 0, 'N'}, bytes.Repeat([]byte{'!'}, len("expected low bytes"))...)}},
	}
	pin := testkit.CachedFileVariants(t, root, files)
	output := filepath.Join(t.TempDir(), "existing.m2")
	if err := os.WriteFile(output, []byte("keep original"), 0600); err != nil {
		t.Fatal(err)
	}
	result, code := invoke(t, "asset", "export", "--snapshot", pin.ID, "--cdn", "--offline", "--file-id", "11", "--content-variant", "low-violence", "--output", output, "--overwrite", "--home", root, "--format=json")
	if code == 0 || result.OK || len(result.Captures) != 0 || result.Result != nil {
		t.Fatal(result, code)
	}
	if result.Error == nil || result.Error.Code != "container.integrity_mismatch" {
		t.Fatalf("not a content integrity rejection: %+v", result.Error)
	}
	actual, err := os.ReadFile(output)
	if err != nil || string(actual) != "keep original" {
		t.Fatal(string(actual), err)
	}
}
