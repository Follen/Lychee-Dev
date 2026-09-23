package command

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
)

func hotfixCommandFixture(t *testing.T) (root, pin, file string, raw []byte) {
	t.Helper()
	root = filepath.Join(t.TempDir(), "workspace")
	if result, code := invoke(t, "init", "--home", root, "--format", "json"); code != 0 {
		t.Fatal(result)
	}
	spec := selection.SelectionSpec{Data: &selection.DataPin{Product: "retail", Region: "cn", Language: "zhCN", FullBuild: "12.1.0.69875", BuildConfig: strings.Repeat("a", 32), CDNConfig: strings.Repeat("b", 32), DefinitionCommit: strings.Repeat("c", 40)}}
	input, err := json.Marshal(spec)
	if err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(t.TempDir(), "selection.json")
	if err := os.WriteFile(path, input, 0600); err != nil {
		t.Fatal(err)
	}
	result, code := invoke(t, "target", "resolve", "--file", path, "--home", root, "--format", "json")
	if code != 0 {
		t.Fatal(result)
	}
	pin = result.Result.(map[string]any)["id"].(string)
	raw = make([]byte, 44)
	copy(raw, "XFTH")
	binary.LittleEndian.PutUint32(raw[4:], 9)
	binary.LittleEndian.PutUint32(raw[8:], 69875)
	for i := 0; i < 3; i++ {
		h := make([]byte, 32)
		copy(h, "XFTH")
		binary.LittleEndian.PutUint32(h[4:], 5)
		binary.LittleEndian.PutUint32(h[8:], uint32(i))
		binary.LittleEndian.PutUint32(h[12:], uint32(100+i))
		binary.LittleEndian.PutUint32(h[16:], 0xabcdef01)
		binary.LittleEndian.PutUint32(h[20:], 42)
		binary.LittleEndian.PutUint32(h[24:], 2)
		h[28] = uint8(i)
		raw = append(raw, h...)
		raw = append(raw, byte(i), 0xff)
	}
	file = filepath.Join(t.TempDir(), "DBCache.bin")
	if err := os.WriteFile(file, raw, 0600); err != nil {
		t.Fatal(err)
	}
	return
}

func TestHotfixCommandArchivePagination(t *testing.T) {
	root, pin, file, raw := hotfixCommandFixture(t)
	query := []string{"data", "hotfix", "--source", "dbcache", "--snapshot", pin, "--dbcache", file, "--table-hash", "ABCDEF01", "--record", "42", "--limit", "1", "--home", root, "--format", "json"}
	first, code := invoke(t, query...)
	if code != 0 || !first.OK || len(first.Captures) != 2 {
		t.Fatalf("first: %d %+v", code, first)
	}
	result := first.Result.(map[string]any)
	derived := result["snapshot"].(string)
	if derived == pin || first.Context["snapshot"] != derived {
		t.Fatal("did not derive a fixed cache identity")
	}
	source := result["source"].(map[string]any)
	sourceID := source["id"].(string)
	page := result["page"].(map[string]any)
	if page["complete"] != false || page["truncated"] != true || page["matched"] != float64(3) || page["nextIndex"] != float64(0) {
		t.Fatal(page)
	}
	entries := page["entries"].([]any)
	if entries[0].(map[string]any)["payloadHex"] != "00ff" {
		t.Fatal(entries)
	}
	for _, item := range first.Captures {
		checked, exit := invoke(t, "evidence", "verify", item.(map[string]any)["id"].(string), "--home", root, "--format", "json")
		if exit != 0 || !checked.OK {
			t.Fatal(checked)
		}
	}
	shown, exit := invoke(t, "target", "show", derived, "--home", root, "--format", "json")
	if exit != 0 {
		t.Fatal(shown)
	}
	changes := shown.Result.(map[string]any)["changes"].(map[string]any)
	if changes["sha256"] != source["blob"].(map[string]any)["sha256"] || changes["provider"] != "dbcache" {
		t.Fatal(changes)
	}
	if err := os.WriteFile(file, []byte("changed client cache"), 0600); err != nil {
		t.Fatal(err)
	}
	second, exit := invoke(t, "data", "hotfix", "--source", "dbcache", "--snapshot", derived, "--from", sourceID, "--after-index", "0", "--limit", "2", "--home", root, "--format", "json")
	if exit != 0 {
		t.Fatal(second)
	}
	secondResult := second.Result.(map[string]any)
	if secondResult["snapshot"] != derived {
		t.Fatal(secondResult)
	}
	last := secondResult["page"].(map[string]any)
	if last["complete"] != false || last["truncated"] != false || len(last["entries"].([]any)) != 2 {
		t.Fatal(last)
	}
	if last["entries"].([]any)[1].(map[string]any)["payloadHex"] != "02ff" {
		t.Fatal(last)
	}
	// A result capture is not a raw cache, and a different cache cannot replace
	// the changes already fixed into the derived target.
	wrong, exit := invoke(t, "data", "hotfix", "--source", "dbcache", "--snapshot", derived, "--from", first.Captures[1].(map[string]any)["id"].(string), "--home", root, "--format", "json")
	if exit != 4 || wrong.Error.Code != "records.hotfix_identity" || wrong.Result != nil {
		t.Fatal(wrong, exit)
	}
	raw[len(raw)-1] = 0xfe
	if err := os.WriteFile(file, raw, 0600); err != nil {
		t.Fatal(err)
	}
	conflict, exit := invoke(t, "data", "hotfix", "--source", "dbcache", "--snapshot", derived, "--dbcache", file, "--home", root, "--format", "json")
	if exit != 4 || conflict.Result != nil || len(conflict.Captures) != 0 {
		t.Fatal(conflict, exit)
	}
}

func TestHotfixCommandErrors(t *testing.T) {
	root, pin, file, raw := hotfixCommandFixture(t)
	for _, extra := range [][]string{
		{}, {"--source", "wago"}, {"--source", "dbcache"},
		{"--source", "dbcache", "--dbcache", file, "--from", "CAP-x"},
		{"--source", "dbcache", "--dbcache", file, "--after-index", "0"},
		{"--source", "dbcache", "--dbcache", file, "--limit", "201"},
		{"--source", "dbcache", "--dbcache", file, "--table-hash", "0xABCDEF01"},
		{"--source", "dbcache", "--dbcache", file, "--table-hash", "xyz"},
		{"--source", "dbcache", "--dbcache", file, "--after-index", "-1"},
		{"--source", "dbcache", "--dbcache", file, "--table", "SpellName", "--table-hash", "00000001"},
		{"--source", "dbcache", "--dbcache", file, "--latest", "--latest"},
		{"--source", "dbcache", "--dbcache", file, "--search", "x"},
		{"--source", "dbcache", "--dbcache", file, "--cursor", "x"},
		{"--source", "dbcache", "--dbcache", file, "--product", "retail"},
		{"--source", "dbcache", "--dbcache", file, "--status", "256"},
		{"--source", "bogus", "--dbcache", file},
		{"--source", "dbcache", "--dbcache", file, "--status", "2", "--status", "3"},
	} {
		args := append([]string{"data", "hotfix", "--snapshot", pin, "--home", root, "--format", "json"}, extra...)
		result, code := invoke(t, args...)
		if code != 2 || result.OK || result.Result != nil {
			t.Fatal(args, result, code)
		}
	}
	for _, tc := range []struct {
		name string
		data []byte
		code string
	}{
		{"tail", append(bytes.Clone(raw), 1), "records.hotfix_format"},
		{"magic", []byte("not a cache"), "records.hotfix_format"},
		{"build", func() []byte { v := bytes.Clone(raw); binary.LittleEndian.PutUint32(v[8:], 12345); return v }(), "records.hotfix_build_mismatch"},
	} {
		t.Run(tc.name, func(t *testing.T) {
			if err := os.WriteFile(file, tc.data, 0600); err != nil {
				t.Fatal(err)
			}
			result, code := invoke(t, "data", "hotfix", "--source", "dbcache", "--snapshot", pin, "--dbcache", file, "--limit", "1", "--home", root, "--format", "json")
			if code != 4 || result.Error.Code != tc.code || result.Result != nil || len(result.Captures) != 0 {
				t.Fatal(result, code)
			}
		})
	}
}

func TestHotfixConcurrentArchiveReads(t *testing.T) {
	root, pin, file, _ := hotfixCommandFixture(t)
	first, code := invoke(t, "data", "hotfix", "--source", "dbcache", "--snapshot", pin, "--dbcache", file, "--home", root, "--format", "json")
	if code != 0 {
		t.Fatal(first)
	}
	result := first.Result.(map[string]any)
	derived := result["snapshot"].(string)
	source := result["source"].(map[string]any)["id"].(string)
	for i := 0; i < 8; i++ {
		t.Run(strconv.Itoa(i), func(t *testing.T) {
			t.Parallel()
			cursor := i % 2
			read, exit := invoke(t, "data", "hotfix", "--source", "dbcache", "--snapshot", derived, "--from", source, "--after-index", strconv.Itoa(cursor), "--home", root, "--format", "json")
			if exit != 0 {
				t.Fatalf("concurrent archive read exit=%d fault=%+v", exit, read.Error)
			}
			r := read.Result.(map[string]any)
			entries := r["page"].(map[string]any)["entries"].([]any)
			if r["snapshot"] != derived || len(entries) != 2-cursor || entries[0].(map[string]any)["index"] != float64(cursor+1) {
				t.Fatal(r)
			}
		})
	}
}

func TestHotfixRejectsCorruptArchivedBytes(t *testing.T) {
	root, pin, file, _ := hotfixCommandFixture(t)
	first, code := invoke(t, "data", "hotfix", "--source", "dbcache", "--snapshot", pin, "--dbcache", file, "--home", root, "--format", "json")
	if code != 0 {
		t.Fatal(first)
	}
	result := first.Result.(map[string]any)
	source := result["source"].(map[string]any)
	digest := source["blob"].(map[string]any)["sha256"].(string)
	if err := os.WriteFile(filepath.Join(root, "blobs", digest[:2], digest[2:]), []byte("corrupt"), 0600); err != nil {
		t.Fatal(err)
	}
	bad, exit := invoke(t, "data", "hotfix", "--source", "dbcache", "--snapshot", result["snapshot"].(string), "--from", source["id"].(string), "--home", root, "--format", "json")
	if exit != 4 || bad.Error.Code != "vault.blob_integrity" || bad.Result != nil || len(bad.Captures) != 0 {
		t.Fatal(bad, exit)
	}
}

// Populate the real workspace's immutable offline definition objects. No global
// HTTP hooks or network requests are used by the CLI integration cases.
func seedHotfixDefinitions(t *testing.T, root, manifest, definition string) {
	t.Helper()
	_, err := vault.WriteMetadata(context.Background(), root, func(s *vault.Store, m *vault.Metadata) (bool, error) {
		base := "https://raw.githubusercontent.com/wowdev/WoWDBDefs/" + strings.Repeat("c", 40) + "/"
		for path, raw := range map[string]string{"manifest.json": manifest, "definitions/TestTable.dbd": definition} {
			ref, err := s.PublishBlob(context.Background(), vault.BlobInput{Reader: strings.NewReader(raw), MaxBytes: 4 << 20})
			if err != nil {
				return false, err
			}
			url := base + path
			digest := sha256.Sum256([]byte(url))
			body, err := json.Marshal(map[string]any{"URL": url, "Blob": ref})
			if err != nil {
				return false, err
			}
			if err := m.CommitDocuments(context.Background(), vault.Mutation{Key: "definition/" + hex.EncodeToString(digest[:]), Value: body}); err != nil {
				return false, err
			}
		}
		return true, nil
	})
	if err != nil {
		t.Fatal(err)
	}
}

const hotfixTestManifest = `[{"tableName":"TestTable","tableHash":"ABCDEF01"}]`
const hotfixTestDefinition = "COLUMNS\nint ID\nint Value\n\nBUILD 12.1.0.69875\n$noninline,id$ID<32>\nValue<16>\n"

func TestHotfixNamedTableAndLatestCLI(t *testing.T) {
	if launcher := os.Getenv("LYCHEEDEV_HOTFIX_LAUNCHER"); launcher != "" {
		t.Logf("using installed hotfix launcher: %s", launcher)
	}
	root, pin, file, raw := hotfixCommandFixture(t)
	args := []string{"data", "hotfix", "--source", "dbcache", "--home", root, "--snapshot", pin, "--dbcache", file, "--table", "TestTable", "--offline", "--format", "json"}
	missing, exit := invokeHotfix(t, args...)
	if exit != 3 || missing.Error.Code != "records.definition_unavailable_offline" || missing.Result != nil {
		t.Fatal(missing, exit)
	}
	seedHotfixDefinitions(t, root, hotfixTestManifest, hotfixTestDefinition)
	// Largest push batch spans two records; the last physical record is older.
	binary.LittleEndian.PutUint32(raw[44+8:], 7)
	binary.LittleEndian.PutUint32(raw[44+34+8:], 7)
	if err := os.WriteFile(file, raw, 0600); err != nil {
		t.Fatal(err)
	}
	first, exit := invokeHotfix(t, append(args, "--latest", "--limit", "1")...)
	if exit != 0 || !first.OK {
		t.Fatal(first, exit)
	}
	r := first.Result.(map[string]any)
	page := r["page"].(map[string]any)
	if page["selectedPush"] != float64(7) || page["matched"] != float64(2) || page["truncated"] != true {
		t.Fatal(page)
	}
	entry := page["entries"].([]any)[0].(map[string]any)
	if entry["decodeState"] != "not_valid" || entry["fields"] != nil {
		t.Fatal(entry)
	}
	definition := r["definition"].(map[string]any)
	if definition["match"] != "build" || definition["build"] != "12.1.0.69875" || definition["fields"] == nil {
		t.Fatal(definition)
	}
	identity := r["sources"].(map[string]any)["identity"].(map[string]any)
	if identity["name"] != "TestTable" || identity["db2FileDataId"] != float64(0) {
		t.Fatal(identity)
	}
	source := r["source"].(map[string]any)["id"].(string)
	derived := r["snapshot"].(string)
	second, exit := invokeHotfix(t, "data", "hotfix", "--source", "dbcache", "--home", root, "--snapshot", derived, "--from", source, "--table", "TestTable", "--latest", "--after-index", "0", "--offline", "--format", "json")
	if exit != 0 {
		t.Fatal(second, exit)
	}
	page = second.Result.(map[string]any)["page"].(map[string]any)
	entries := page["entries"].([]any)
	if len(entries) != 1 || page["matched"] != float64(2) || page["complete"] != false || page["truncated"] != false {
		t.Fatal(page)
	}
	entry = entries[0].(map[string]any)
	fields := entry["fields"].(map[string]any)
	if entry["decodeState"] != "decoded" || entry["payloadHex"] != "01ff" || fields["ID"] != float64(42) || fields["Value"] != float64(-255) {
		t.Fatal(entry)
	}
	for _, capture := range second.Captures {
		verified, exit := invokeHotfix(t, "evidence", "verify", capture.(map[string]any)["id"].(string), "--home", root, "--format", "json")
		if exit != 0 || !verified.OK {
			t.Fatal(verified, exit)
		}
	}
	// The same original bytes remain accessible without a definition selection.
	rawResult, exit := invokeHotfix(t, "data", "hotfix", "--source", "dbcache", "--home", root, "--snapshot", derived, "--from", source, "--table-hash", "ABCDEF01", "--offline", "--format", "json")
	if exit != 0 {
		t.Fatal(rawResult, exit)
	}
	entry = rawResult.Result.(map[string]any)["page"].(map[string]any)["entries"].([]any)[1].(map[string]any)
	if entry["fields"] != nil || entry["decodeState"] != nil {
		t.Fatal(entry)
	}
}

func invokeHotfix(t *testing.T, args ...string) (Envelope, int) {
	t.Helper()
	// LAUNCHER alone executes a native binary directly; NODE+LAUNCHER drives
	// the npm launcher (bin/lycheedev.mjs) through node.
	node, launcher := os.Getenv("LYCHEEDEV_HOTFIX_NODE"), os.Getenv("LYCHEEDEV_HOTFIX_LAUNCHER")
	if node == "" && launcher == "" {
		return invoke(t, args...)
	}
	if launcher == "" {
		t.Fatal("installed Hotfix test requires LYCHEEDEV_HOTFIX_LAUNCHER")
	}
	if node == "" {
		return invokeExecutable(t, launcher, args...)
	}
	return invokeExecutable(t, node, append([]string{launcher}, args...)...)
}

func TestHotfixDefinitionFailures(t *testing.T) {
	for _, tc := range []struct {
		name, manifest, definition, code string
		exit                             int
	}{
		{"collision", `[{"tableName":"TestTable","tableHash":"ABCDEF01"},{"tableName":"Other","tableHash":"ABCDEF01"}]`, hotfixTestDefinition, "schema.ambiguous_definition", 4},
		{"wrong-build", hotfixTestManifest, strings.ReplaceAll(hotfixTestDefinition, "69875", "60000"), "schema.definition_not_found", 3},
		{"layout-only", hotfixTestManifest, strings.ReplaceAll(hotfixTestDefinition, "BUILD 12.1.0.69875", "LAYOUT 00000001"), "schema.definition_not_found", 3},
		{"truncated", hotfixTestManifest, strings.ReplaceAll(hotfixTestDefinition, "Value<16>", "Value<32>"), "records.hotfix_fields", 4},
		{"trailing", hotfixTestManifest, strings.ReplaceAll(hotfixTestDefinition, "Value<16>", "Value<8>"), "records.hotfix_fields", 4},
	} {
		t.Run(tc.name, func(t *testing.T) {
			root, pin, file, _ := hotfixCommandFixture(t)
			seedHotfixDefinitions(t, root, tc.manifest, tc.definition)
			result, exit := invoke(t, "data", "hotfix", "--source", "dbcache", "--home", root, "--snapshot", pin, "--dbcache", file, "--table", "TestTable", "--offline", "--format", "json")
			if exit != tc.exit || result.Error.Code != tc.code || result.Result != nil || len(result.Captures) != 0 {
				t.Fatal(result, exit)
			}
		})
	}
}
