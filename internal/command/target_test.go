package command

import (
	"crypto/md5"
	"fmt"
	"github.com/follenfang/lycheedev/internal/testkit"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestTargetResolveFromClientReturnsReusablePinAndEvidence(t *testing.T) {
	game := t.TempDir()
	client := filepath.Join(game, "selected-client")
	if err := os.Mkdir(client, 0700); err != nil {
		t.Fatal(err)
	}
	for name, value := range map[string]string{".flavor.info": "wow", "version.txt": "12.1.0.69875"} {
		if err := os.WriteFile(filepath.Join(client, name), []byte(value), 0600); err != nil {
			t.Fatal(err)
		}
	}
	configs := []string{
		fmt.Sprintf("root = %s\nencoding = %s %s\nencoding-size = 123 456\nbuild-uid = wow\n", strings.Repeat("1", 32), strings.Repeat("2", 32), strings.Repeat("3", 32)),
		"archives = fixture\n",
	}
	var keys []string
	for _, value := range configs {
		key := fmt.Sprintf("%x", md5.Sum([]byte(value)))
		keys = append(keys, key)
		directory := filepath.Join(game, "Data", "config", key[:2], key[2:4])
		if err := os.MkdirAll(directory, 0700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(directory, key), []byte(value), 0600); err != nil {
			t.Fatal(err)
		}
	}
	catalog := fmt.Sprintf("Product!STRING:0|Version!STRING:0|Build Key!HEX:16|CDN Key!HEX:16|Active!DEC:1\nwow|12.1.0.69875|%s|%s|1\n", keys[0], keys[1])
	if err := os.WriteFile(filepath.Join(game, ".build.info"), []byte(catalog), 0600); err != nil {
		t.Fatal(err)
	}
	workspace := filepath.Join(t.TempDir(), "workspace")
	if result, code := invoke(t, "init", "--home", workspace, "--format=json"); code != 0 || !result.OK {
		t.Fatal(result)
	}
	args := []string{"target", "resolve", "--installation", client, "--region", "cn", "--locale", "zhCN", "--definitions", strings.Repeat("d", 40), "--offline", "--home", workspace, "--format=json"}
	result, code := invoke(t, args...)
	if code != 0 || !result.OK {
		t.Fatal(result, code)
	}
	pin := resultMap(t, result)
	data := pin["data"].(map[string]any)
	if data["product"] != "retail" || data["fullBuild"] != "12.1.0.69875" || data["buildConfig"] != keys[0] || result.Context["snapshot"] != pin["id"] || result.Context["installation"] != testkit.CanonicalPath(t, game) || len(result.Captures) != 1 {
		t.Fatal(result)
	}
	shown, code := invoke(t, "target", "show", pin["id"].(string), "--home", workspace, "--format=json")
	if code != 0 || resultMap(t, shown)["id"] != pin["id"] {
		t.Fatal(shown)
	}
	capture := result.Captures[0].(map[string]any)
	verified, code := invoke(t, "evidence", "verify", capture["id"].(string), "--home", workspace, "--format=json")
	if code != 0 || !verified.OK {
		t.Fatal(verified)
	}
	repeated, code := invoke(t, args...)
	if code != 0 || resultMap(t, repeated)["id"] != pin["id"] {
		t.Fatal("nonrepeatable pin", repeated)
	}
}

func TestTargetPreparationArgumentErrorsDoNotTouchWorkspace(t *testing.T) {
	for _, args := range [][]string{
		{"--product", "retail", "--region", "cn"},
		{"--product", "retail", "--region", "cn", "--locale", "zhCN", "--build", "latest"},
		{"--product", "retail", "--installation", "missing"},
		{"--product", "unsupported", "--region", "cn", "--locale", "zhCN"},
		{"--build", "12.1.0.69875"},
		{"--file", "missing", "--product", "retail"},
		{"--file", "missing", "--build", "12.1.0.69875"},
		{"--installation", "missing", "--region", "cn"},
		{"--installation", "missing", "--locale", "zhCN"},
		{"--installation", "missing", "--region", "window", "--locale", "zhCN"},
		{"--file", "missing", "--installation", "missing"},
		{"--file", "missing", "--locale", "zhCN"},
		{"--file", "missing", "--region", "cn"},
		{"--file", "missing", "--from", "PIN-x"},
		{"--file", "missing", "--definitions", strings.Repeat("a", 40)},
		{"--file", "missing", "--offline"},
	} {
		root := filepath.Join(t.TempDir(), "absent")
		argv := append([]string{"target", "resolve", "--home", root, "--format=json"}, args...)
		response, code := invoke(t, argv...)
		if code != 2 || response.OK || response.Error.Code != "command.invalid_arguments" {
			t.Fatal(argv, response, code)
		}
		if _, err := os.Stat(root); !os.IsNotExist(err) {
			t.Fatal("invalid request touched workspace", err)
		}
	}
}

func TestRemoteTargetOfflineNeedsObservation(t *testing.T) {
	root := filepath.Join(t.TempDir(), "workspace")
	if result, code := invoke(t, "init", "--home", root, "--format=json"); code != 0 {
		t.Fatal(result)
	}
	result, code := invoke(t, "target", "resolve", "--product", "retail", "--region", "cn", "--locale", "zhCN", "--definitions", strings.Repeat("d", 40), "--offline", "--home", root, "--format=json")
	if code != 3 || result.OK || result.Error.Code != "records.remote_unavailable_offline" || len(result.Captures) != 0 {
		t.Fatal(result, code)
	}
}
