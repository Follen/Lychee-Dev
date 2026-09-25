package command

import (
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestHelpAndDescribeExposeTheImplementedContract(t *testing.T) {
	all, code := invoke(t, "--help", "--format=json")
	if code != 0 || !all.OK {
		t.Fatalf("help: code=%d response=%+v", code, all)
	}
	allPayload := resultMap(t, all)
	allCommands := commandList(t, allPayload)
	if len(allCommands) != len(commandContracts) {
		t.Fatalf("help command count = %d, want %d", len(allCommands), len(commandContracts))
	}
	for _, command := range allCommands {
		if _, ok := command["path"].(string); !ok {
			t.Fatalf("help command has no path: %#v", command)
		}
		if _, ok := command["flags"].([]any); !ok {
			t.Fatalf("help command has no flags: %#v", command)
		}
		if _, ok := command["arguments"].([]any); !ok {
			t.Fatalf("help command has no arguments: %#v", command)
		}
	}

	described, code := invoke(t, "describe", "--format=json")
	if code != 0 || !described.OK {
		t.Fatalf("describe: code=%d response=%+v", code, described)
	}
	describedCommands := commandList(t, resultMap(t, described))
	if len(describedCommands) != len(commandContracts) {
		t.Fatalf("describe command count = %d, want %d", len(describedCommands), len(commandContracts))
	}

	for _, args := range [][]string{{"live", "run", "--help"}, {"live", "resume", "--help"}, {"source", "query", "--help"}} {
		response, code := invoke(t, append(args, "--format=json")...)
		if code != 0 || !response.OK {
			t.Fatalf("%v: code=%d response=%+v", args, code, response)
		}
		commands := commandList(t, resultMap(t, response))
		if len(commands) != 1 || commands[0]["path"] != args[0]+" "+args[1] {
			t.Fatalf("%v returned %#v", args, commands)
		}
	}

	resumeHelp, code := invoke(t, "live", "resume", "--help", "--format=json")
	if code != 0 || !resumeHelp.OK {
		t.Fatalf("resume help: code=%d response=%+v", code, resumeHelp)
	}
	resume := commandList(t, resultMap(t, resumeHelp))[0]
	if got := stringList(t, resume["flags"]); len(got) != 2 || got[0] != "--home <root>" || got[1] != "--format text|json|jsonl" {
		t.Fatalf("live resume flags = %#v", got)
	}
	if got := stringList(t, resume["arguments"]); len(got) != 1 || got[0] != "<operation-id>" {
		t.Fatalf("live resume arguments = %#v", got)
	}
}

func TestConnectAndInstanceContractsComeFromTheSameTable(t *testing.T) {
	connectHelp, code := invoke(t, "live", "connect", "--help", "--format=json")
	if code != 0 || !connectHelp.OK {
		t.Fatalf("connect help: code=%d response=%+v", code, connectHelp)
	}
	commands := commandList(t, resultMap(t, connectHelp))
	if len(commands) != 1 || commands[0]["path"] != "live connect" || commands[0]["mutates"] != true {
		t.Fatalf("live connect definition: %#v", commands)
	}
	got := stringList(t, commands[0]["flags"])
	want := []string{"--home <root>", "--format text|json|jsonl", "--project <directory>",
		"--snapshot <pin>", "--character <name>", "--realm <realm>", "--pid <pid>",
		"--installation <client>", "--session <session-id>", "--capture-area <window|x,y,width,height>"}
	if len(got) != len(want) {
		t.Fatalf("live connect flags = %#v", got)
	}
	for i := range want {
		if got[i] != want[i] {
			t.Fatalf("live connect flags = %#v, want %#v", got, want)
		}
	}
	instancesHelp, code := invoke(t, "live", "instances", "--help", "--format=json")
	if code != 0 || !instancesHelp.OK {
		t.Fatalf("instances help: code=%d response=%+v", code, instancesHelp)
	}
	instances := commandList(t, resultMap(t, instancesHelp))
	if len(instances) != 1 || instances[0]["path"] != "live instances" || instances[0]["mutates"] != true {
		t.Fatalf("live instances definition: %#v", instances)
	}
	if got := stringList(t, instances[0]["flags"]); len(got) != 3 || got[2] != "--installation <client-or-game-root>" {
		t.Fatalf("live instances flags = %#v", got)
	}
	described, code := invoke(t, "describe", "--format=json")
	if code != 0 || !described.OK {
		t.Fatalf("describe: code=%d response=%+v", code, described)
	}
	for _, definition := range commandList(t, resultMap(t, described)) {
		if definition["path"] == "live connect" || definition["path"] == "live instances" {
			for _, name := range []string{"summary", "flags", "arguments"} {
				if definition[name] == nil {
					t.Fatalf("describe lost %s of %#v", name, definition)
				}
			}
		}
	}
}

func TestLiveConnectArgumentAdmission(t *testing.T) {
	// These calls fail on the missing snapshot/session before any window is
	// enumerated, so no game input can ever happen from contract tests.
	for _, args := range [][]string{
		{"live", "connect", "--format=json"},
		{"live", "connect", "--snapshot", "PIN-missing", "--character", "bad\nname", "--format=json"},
		{"live", "connect", "--snapshot", "PIN-missing", "--pid", "0", "--format=json"},
		{"live", "connect", "--snapshot", "PIN-missing", "--capture-area", "1,2,3", "--format=json"},
		{"live", "connect", "--snapshot", "PIN-missing", "--nonce", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "--format=json"},
	} {
		if result, code := invoke(t, args...); code != 2 || result.OK {
			t.Fatalf("invalid %v: %+v code=%d", args, result, code)
		}
	}
	missing := filepath.Join(t.TempDir(), "absent-home")
	for _, args := range [][]string{
		{"live", "connect", "--snapshot", "PIN-missing"},
		{"live", "connect", "--session", "SESSION-missing"},
		{"live", "connect", "--snapshot", "PIN-missing", "--installation", t.TempDir()},
	} {
		argv := append(append([]string{}, args...), "--home", missing, "--format=json")
		if result, code := invoke(t, argv...); code == 0 || result.OK || result.Result != nil {
			t.Fatalf("unresolved %v: %+v code=%d", args, result, code)
		}
	}
	if _, err := os.Stat(missing); !os.IsNotExist(err) {
		t.Fatal("connect initialized workspace", err)
	}
	for _, flag := range []string{"--session=S", "--snapshot=P", "--capture-area=window", "--installation=D", "--pid=1", "--character=C", "--realm=R"} {
		if result, code := invoke(t, "version", flag, "--format=json"); code != 2 || result.OK {
			t.Fatal("ignored connect flag", flag)
		}
	}
}

func TestWorkspaceVerbContractsAreAdvertised(t *testing.T) {
	described, code := invoke(t, "describe", "--format=json")
	if code != 0 || !described.OK {
		t.Fatalf("describe: code=%d response=%+v", code, described)
	}
	advertised := map[string]bool{}
	for _, definition := range commandList(t, resultMap(t, described)) {
		advertised[definition["path"].(string)] = true
	}
	for _, path := range []string{
		"target list", "target add", "target remove", "target available",
		"doctor", "config show", "config set",
		"cache status", "cache verify", "cache prune",
		"evidence list",
	} {
		if !advertised[path] {
			t.Fatalf("describe does not advertise %q", path)
		}
	}
	for _, path := range []string{"evidence keep", "evidence remove"} {
		if !advertised[path] {
			t.Fatalf("describe does not advertise %q", path)
		}
	}

	addHelp, code := invoke(t, "target", "add", "--help", "--format=json")
	if code != 0 || !addHelp.OK {
		t.Fatalf("target add help: code=%d response=%+v", code, addHelp)
	}
	commands := commandList(t, resultMap(t, addHelp))
	if len(commands) != 1 || commands[0]["path"] != "target add" || commands[0]["mutates"] != true {
		t.Fatalf("target add definition: %#v", commands)
	}
	got := stringList(t, commands[0]["flags"])
	want := map[string]bool{"--home <root>": false, "--format text|json|jsonl": false, "--product <retail|classic|titan|forever>": false,
		"--region <us|eu|cn|kr|tw>": false, "--locale <locale>": false, "--build <full-build>": false,
		"--installation <client-directory>": false, "--remote": false, "--replace": false}
	for _, flag := range got {
		if _, known := want[flag]; known {
			want[flag] = true
		}
	}
	for flag, seen := range want {
		if !seen {
			t.Fatalf("target add help lost %s: %#v", flag, got)
		}
	}
	if arguments := stringList(t, commands[0]["arguments"]); len(arguments) != 1 || arguments[0] != "<name>" {
		t.Fatalf("target add arguments = %#v", arguments)
	}

	initHelp, code := invoke(t, "init", "--help", "--format=json")
	if code != 0 || !initHelp.OK {
		t.Fatalf("init help: code=%d response=%+v", code, initHelp)
	}
	commands = commandList(t, resultMap(t, initHelp))
	initFlags := strings.Join(stringList(t, commands[0]["flags"]), " ")
	for _, flag := range []string{"--plan", "--fresh", "--resume"} {
		if !strings.Contains(initFlags, flag) {
			t.Fatalf("init help lost %s: %#v", flag, initFlags)
		}
	}
}

func TestWorkspaceVerbArgumentAdmission(t *testing.T) {
	for _, args := range [][]string{
		{"target", "add"},
		{"target", "add", "name", "--region", "cn", "--locale", "zhCN", "--remote"},
		{"target", "add", "name", "--product", "retail", "--locale", "zhCN", "--remote"},
		{"target", "add", "name", "--product", "retail", "--region", "cn", "--remote"},
		{"target", "add", "name", "--product", "unsupported", "--region", "cn", "--locale", "zhCN", "--remote"},
		{"target", "add", "name", "--product", "retail", "--region", "window", "--locale", "zhCN", "--remote"},
		{"target", "add", "name", "--product", "retail", "--region", "cn", "--locale", "zhCN", "--remote", "--build", "latest"},
		{"target", "add", "name", "--product", "retail", "--region", "cn", "--locale", "zhCN"},
		{"target", "add", "name", "--product", "retail", "--region", "cn", "--locale", "zhCN", "--installation", "client", "--remote"},
		{"target", "remove"},
		{"target", "available"},
		{"target", "available", "--region", "window"},
		{"target", "available", "--offline"},
		{"cache", "prune", "--target-bytes", "-1"},
		{"cache", "prune", "--max-objects", "-1"},
		{"cache", "status", "--dry-run"},
		{"config", "set"},
		{"config", "set", "--cache-max-bytes", "-5"},
		{"config", "set", "--download-workers", "257"},
		{"config", "show", "--cache-max-bytes", "100"},
		{"init", "--plan", "--fresh"},
		{"init", "--plan", "--resume"},
		{"init", "--fresh", "--resume"},
		{"init", "--plan", "--fresh", "--resume"},
		{"doctor", "--remote"},
		{"evidence", "list", "--limit", "201"},
		{"evidence", "keep"},
		{"evidence", "keep", "CAP-example", "extra"},
		{"evidence", "remove"},
		{"evidence", "remove", "CAP-example", "extra"},
	} {
		argv := append(append([]string{}, args...), "--format=json")
		// Malformed target fields reach the module validator and carry the
		// selection.target_format fault; everything else is rejected by the
		// parser as command.invalid_arguments. Both are exit 2 rejections.
		if result, code := invoke(t, argv...); code != 2 || result.OK || result.Error == nil {
			t.Fatalf("invalid %v accepted: code=%d response=%+v", argv, code, result)
		}
	}
}

func TestSubcommandHelpDoesNotMakeUnknownCommandsUsable(t *testing.T) {
	// Unknown verbs remain unrouted even when a caller asks for help.
	for _, args := range [][]string{{"missing", "--help"}, {"live", "unknown", "--help"}} {
		response, code := invoke(t, append(args, "--format=json")...)
		if code != 2 || response.OK || response.Error == nil || response.Error.Code != "command.invalid_arguments" {
			t.Fatalf("%v: code=%d response=%+v", args, code, response)
		}
	}
}

func TestLiveCommandFlagAdmissionComesFromItsContract(t *testing.T) {
	for _, args := range [][]string{
		{"live", "run"},
		{"live", "ack"},
		{"live", "status"},
		{"live", "session"},
	} {
		response, code := invoke(t, append(args, "--format=json")...)
		if code != 2 || response.OK || response.Error == nil || response.Error.Code != "command.invalid_arguments" {
			t.Fatalf("missing positional argument accepted for %v: code=%d response=%+v", args, code, response)
		}
	}

	for _, args := range [][]string{
		{"live", "run", "OP-loaded", "--home", "root", "--help", "--format=json"},
		{"live", "reload", "--session", "SESSION", "--request", "reload-1", "--home", "root", "--help", "--format=json"},
		{"live", "probe", "load", "--session", "SESSION", "--request", "load-1", "--probe", "probe-name", "--account", "ACCOUNT", "--home", "root", "--help", "--format=json"},
		{"live", "bugs", "--session", "SESSION", "--request", "bugs-1", "--count", "10", "--account", "ACCOUNT", "--home", "root", "--help", "--format=json"},
		{"live", "ack", "OP-verified", "--home", "root", "--help", "--format=json"},
		{"live", "resume", "OP-existing", "--home", "root", "--help", "--format=json"},
	} {
		response, code := invoke(t, args...)
		if code != 0 || !response.OK {
			t.Fatalf("accepted command flags %v: code=%d response=%+v", args, code, response)
		}
	}

	for _, flag := range []string{"--installation=other", "--pid=7", "--character=Other", "--realm=Other", "--nonce=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "--snapshot=PIN-other", "--region=window"} {
		response, code := invoke(t, "live", "run", "OP-loaded", flag, "--format=json")
		if code != 2 || response.OK || response.Error == nil || !strings.Contains(response.Error.Message, flag[:strings.IndexByte(flag, '=')]) {
			t.Fatalf("live run accepted %s: code=%d response=%+v", flag, code, response)
		}
	}
	for _, flag := range []string{"--region=window", "--installation=other", "--snapshot=PIN-other", "--session=other", "--account=other", "--pid=7"} {
		response, code := invoke(t, "live", "resume", "OP-existing", flag, "--format=json")
		if code != 2 || response.OK || response.Error == nil || !strings.Contains(response.Error.Message, flag[:strings.IndexByte(flag, '=')]) {
			t.Fatalf("live resume accepted %s: code=%d response=%+v", flag, code, response)
		}
	}
}

func resultMap(t *testing.T, response Envelope) map[string]any {
	t.Helper()
	result, ok := response.Result.(map[string]any)
	if !ok {
		t.Fatalf("result = %#v", response.Result)
	}
	return result
}

func commandList(t *testing.T, result map[string]any) []map[string]any {
	t.Helper()
	values, ok := result["commands"].([]any)
	if !ok {
		t.Fatalf("commands = %#v", result["commands"])
	}
	commands := make([]map[string]any, len(values))
	for i, value := range values {
		command, ok := value.(map[string]any)
		if !ok {
			t.Fatalf("command %d = %#v", i, value)
		}
		commands[i] = command
	}
	return commands
}

func stringList(t *testing.T, value any) []string {
	t.Helper()
	values, ok := value.([]any)
	if !ok {
		t.Fatalf("string list = %#v", value)
	}
	result := make([]string, len(values))
	for i, value := range values {
		result[i], ok = value.(string)
		if !ok {
			t.Fatalf("string %d = %#v", i, value)
		}
	}
	return result
}

func TestCommandHelpDoesNotTouchWorkspace(t *testing.T) {
	root := filepath.Join(t.TempDir(), "absent")
	t.Setenv("LYCHEEDEV_HOME", root)
	response, code := invoke(t, "live", "resume", "--help", "--format=json")
	if code != 0 || !response.OK {
		t.Fatalf("code=%d response=%+v", code, response)
	}
	if _, err := os.Stat(root); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("help touched workspace: %v", err)
	}
}

func TestDataVerbContractsAreAdvertisedAndRouted(t *testing.T) {
	expected := []string{
		"data hotfix",
		"data db2 schema", "data db2 search", "data db2 foreign-key", "data db2 stream",
		"data spell info", "data spell auras", "data spell summons",
		"data item get", "data item models", "data item geosets", "data item textures",
		"data creature display", "data creature model", "data encounter get",
		"data decor list", "data decor get",
	}
	described, code := invoke(t, "describe", "--format=json")
	if code != 0 || !described.OK {
		t.Fatalf("describe: code=%d response=%+v", code, described)
	}
	advertised := map[string]bool{}
	for _, definition := range commandList(t, resultMap(t, described)) {
		advertised[definition["path"].(string)] = true
	}
	for _, path := range expected {
		if !advertised[path] {
			t.Fatalf("describe does not advertise %q", path)
		}
	}

	// Longest path wins: the schema verb answers for its own path while the
	// row read keeps its two-word contract.
	schemaHelp, code := invoke(t, "data", "db2", "schema", "--help", "--format=json")
	if code != 0 || !schemaHelp.OK {
		t.Fatalf("schema help: code=%d response=%+v", code, schemaHelp)
	}
	commands := commandList(t, resultMap(t, schemaHelp))
	if len(commands) != 1 || commands[0]["path"] != "data db2 schema" {
		t.Fatalf("schema help returned %#v", commands)
	}
	got := stringList(t, commands[0]["flags"])
	want := map[string]bool{"--snapshot <pin>": false, "--project <directory>": false, "--cdn": false, "--offline": false}
	for _, flag := range got {
		if _, known := want[flag]; known {
			want[flag] = true
		}
	}
	for flag, seen := range want {
		if !seen {
			t.Fatalf("schema help lost %s: %#v", flag, got)
		}
	}
	if arguments := stringList(t, commands[0]["arguments"]); len(arguments) != 1 || arguments[0] != "<Table>" {
		t.Fatalf("schema arguments = %#v", arguments)
	}
	rowHelp, code := invoke(t, "data", "db2", "--help", "--format=json")
	if code != 0 || !rowHelp.OK {
		t.Fatalf("row help: code=%d response=%+v", code, rowHelp)
	}
	commands = commandList(t, resultMap(t, rowHelp))
	if len(commands) != 1 || commands[0]["path"] != "data db2" {
		t.Fatalf("row help returned %#v", commands)
	}

	hotfixHelp, code := invoke(t, "data", "hotfix", "--help", "--format=json")
	if code != 0 || !hotfixHelp.OK {
		t.Fatalf("hotfix help: code=%d response=%+v", code, hotfixHelp)
	}
	commands = commandList(t, resultMap(t, hotfixHelp))
	if len(commands) != 1 || commands[0]["path"] != "data hotfix" {
		t.Fatalf("hotfix help returned %#v", commands)
	}
	hotfixFlags := strings.Join(stringList(t, commands[0]["flags"]), " ")
	for _, flag := range []string{"--source <wago|dbcache|raidbots>", "--dbcache <DBCache.bin>", "--raidbots <DBCache.bin>", "--record <n>", "--cursor <opaque>", "--encoding csv"} {
		if !strings.Contains(hotfixFlags, flag) {
			t.Fatalf("hotfix help lost %s: %#v", flag, hotfixFlags)
		}
	}
	// The retired row-read flags no longer answer for the hotfix route.
	for _, args := range [][]string{
		{"data", "hotfix", "--file", "DBCache.bin"},
		{"data", "hotfix", "--id", "1"},
		{"data", "db2", "schema", "Map", "--table", "Other"},
		{"data", "asset", "export", "--file-id", "1", "--encoding", "csv"},
	} {
		if result, code := invoke(t, append(args, "--format=json")...); code != 2 || result.OK {
			t.Fatalf("accepted retired or foreign flag %v: %d %+v", args, code, result)
		}
	}
}
