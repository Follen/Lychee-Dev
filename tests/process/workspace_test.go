package process_test

import (
	"bytes"
	"context"
	"encoding/json"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/codebase"
	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/testkit"
	"github.com/follenfang/lycheedev/internal/vault"
)

func TestNativeWorkspaceAcrossProcesses(t *testing.T) {
	temp := t.TempDir()
	binary := filepath.Join(temp, "lycheedev")
	if runtime.GOOS == "windows" {
		binary += ".exe"
	}
	build := exec.Command("go", "build", "-o", binary, "../../cmd/lycheedev")
	if out, err := build.CombinedOutput(); err != nil {
		t.Fatalf("build: %v\n%s", err, out)
	}
	root := filepath.Join(temp, "isolated-workspace")
	run := func(args ...string) map[string]any {
		t.Helper()
		if !(len(args) > 1 && args[0] == "project" && (args[1] == "init" || args[1] == "status")) {
			args = append(args, "--home", root)
		}
		args = append(args, "--format", "json")
		cmd := exec.Command(binary, args...)
		out, err := cmd.CombinedOutput()
		if err != nil {
			t.Fatalf("%v: %v\n%s", args, err, out)
		}
		var response map[string]any
		if err := json.Unmarshal(out, &response); err != nil {
			t.Fatal(err)
		}
		if response["ok"] != true {
			t.Fatalf("%v", response)
		}
		return response
	}
	run("init")
	// Two real native processes race on one explicit output; only one may
	// publish. Both independently resolve and verify the cached CASC object.
	body := bytes.Repeat([]byte{0, 255, 3, 10}, 80000)
	assetPin := testkit.CachedAsset(t, root, body)
	exportPath := filepath.Join(temp, "export.bin")
	exportArgs := []string{"asset", "export", "--snapshot", assetPin.ID, "--cdn", "--offline", "--file-id", "11", "--output", exportPath, "--home", root, "--format", "json"}
	var processes [2]*exec.Cmd
	var outputs [2]bytes.Buffer
	for i := range processes {
		processes[i] = exec.Command(binary, exportArgs...)
		processes[i].Stdout, processes[i].Stderr = &outputs[i], &outputs[i]
		if err := processes[i].Start(); err != nil {
			t.Fatal(err)
		}
	}
	winners := 0
	for i, process := range processes {
		err := process.Wait()
		var response map[string]any
		if json.Unmarshal(outputs[i].Bytes(), &response) != nil {
			t.Fatalf("export process: %v\n%s", err, outputs[i].String())
		}
		if err == nil {
			winners++
			if response["ok"] != true {
				t.Fatal(response)
			}
			for _, capture := range response["captures"].([]any) {
				run("evidence", "verify", capture.(map[string]any)["id"].(string))
			}
		} else if process.ProcessState.ExitCode() != 3 || response["error"].(map[string]any)["code"] != "records.export_conflict" {
			t.Fatalf("unexpected export failure: %v\n%s", err, outputs[i].String())
		}
	}
	if winners != 1 {
		t.Fatalf("export winners=%d", winners)
	}
	exportedBytes, err := os.ReadFile(exportPath)
	if err != nil || !bytes.Equal(exportedBytes, body) {
		t.Fatal("incomplete/mixed export", err)
	}
	spec := filepath.Join(temp, "selection.json")
	if err := os.WriteFile(spec, []byte(`{"source":{"repository":"fixture","product":"retail","requestedRef":"fixed","exactCommit":"`+strings.Repeat("a", 40)+`","parserRevision":"1"}}`), 0600); err != nil {
		t.Fatal(err)
	}
	first := run("target", "resolve", "--file", spec)["result"].(map[string]any)
	id := first["id"].(string)
	second := run("target", "show", id)["result"].(map[string]any)
	if second["id"] != id {
		t.Fatal("pin changed across CLI processes")
	}

	// Seed one unfinished operation in the game journal,
	// close the database, then observe it only through a fresh native process.
	ctx := context.Background()
	store, err := vault.OpenStore(root)
	if err != nil {
		t.Fatal(err)
	}
	metadata, err := store.OpenMetadata(ctx)
	if err != nil {
		t.Fatal(err)
	}
	book := journal.OpenBook(metadata)
	work, err := book.BeginWork(ctx, journal.WorkIntent{Kind: "probe", Resource: "synthetic-window", Snapshot: id, Session: "WIN-fixture", Request: json.RawMessage(`{"code":"return 1"}`)})
	if err != nil {
		t.Fatal(err)
	}
	archive := evidence.OpenArchive(store, metadata)
	capture, err := archive.CommitCapture(ctx, evidence.CaptureDraft{Reader: strings.NewReader("exact bytes\r\n"), MaxBytes: 1024, MediaType: "text/plain", Complete: true, Provenance: evidence.Provenance{Kind: "fixture", Locator: "test-input", Snapshot: id, OperationID: work.OperationID}})
	if err != nil {
		t.Fatal(err)
	}
	if err := metadata.Close(); err != nil {
		t.Fatal(err)
	}
	status := run("live", "status", work.OperationID)["result"].(map[string]any)
	if status["status"] != "pending" || status["operationId"] != work.OperationID || status["snapshot"] != id || status["complete"] != false || status["cleanup"] != "pending" {
		t.Fatalf("wrong status %v", status)
	}
	if status["report"].(map[string]any)["state"] != "unavailable" || status["observation"] != nil || status["intent"] != nil || status["stage"] != nil {
		t.Fatalf("status exposed protocol machinery or invented a report: %v", status)
	}
	verified := run("evidence", "verify", capture.ID)["result"].(map[string]any)
	if verified["id"] != capture.ID || verified["complete"] != true {
		t.Fatalf("wrong capture %v", verified)
	}
	// This is a process/storage contract test, not a simulated live success.
	metadata, err = store.OpenMetadata(ctx)
	if err != nil {
		t.Fatal(err)
	}
	defer metadata.Close()
	observed, err := journal.OpenBook(metadata).InspectWork(ctx, work.OperationID)
	if err != nil || observed.Stage != "prepared" || observed.Generation != 1 {
		t.Fatalf("read command advanced live work: %+v %v", observed, err)
	}

	// Offline source inspection crosses the actual CLI, pin store and archive.
	// Only Git fixture construction is in-process test setup; no network needed.
	mirror := filepath.Join(root, "mirrors", "wow-ui-source.git")
	git := func(input string, args ...string) string {
		t.Helper()
		cmd := exec.Command("git", append([]string{"-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "--git-dir=" + mirror}, args...)...)
		cmd.Stdin = strings.NewReader(input)
		out, err := cmd.CombinedOutput()
		if err != nil {
			t.Fatalf("git %v: %v\n%s", args, err, out)
		}
		return strings.TrimSpace(string(out))
	}
	git("", "init", "--bare", mirror)
	sourceBytes := "first\r\nsecond\r\nthird"
	blobID := git(sourceBytes, "hash-object", "-w", "--stdin")
	treeID := git("100644 blob "+blobID+"\tfixture.lua\n", "mktree")
	commitID := git("fixture", "commit-tree", treeID)
	sourceSpec := `{"source":{"repository":"wow-ui-source","product":"retail","requestedRef":"fixture","exactCommit":"` + commitID + `","parserRevision":"` + codebase.ParserRevision + `"}}`
	if err := os.WriteFile(spec, []byte(sourceSpec), 0600); err != nil {
		t.Fatal(err)
	}
	sourcePin := run("target", "resolve", "--file", spec)["result"].(map[string]any)["id"].(string)
	project := filepath.Join(temp, "source-project")
	if err := os.Mkdir(project, 0700); err != nil {
		t.Fatal(err)
	}
	run("project", "init", "--path", project, "--product", "retail")
	run("project", "lock", "--path", project, "--snapshot", sourcePin)
	source := run("source", "inspect", "--project", project, "--path", "fixture.lua", "--line", "2", "--count", "1")
	if source["context"].(map[string]any)["snapshot"] != sourcePin {
		t.Fatal("project did not select fixed source", source)
	}
	excerpt := source["result"].(map[string]any)
	if excerpt["text"] != "second\r\n" || excerpt["commit"] != commitID {
		t.Fatalf("wrong excerpt: %v", excerpt)
	}
	captures := source["captures"].([]any)
	if len(captures) != 1 {
		t.Fatalf("missing source evidence: %v", captures)
	}
	sourceCapture := captures[0].(map[string]any)["id"].(string)
	run("evidence", "verify", sourceCapture)
	ref, original, err := evidence.OpenArchive(store, metadata).FetchCapture(ctx, sourceCapture, 1024)
	if err != nil || string(original) != sourceBytes || ref.Provenance.SourceCommit != commitID || ref.Provenance.Snapshot != sourcePin {
		t.Fatalf("source provenance/bytes: %+v %q %v", ref, original, err)
	}
	// The fixture is intentionally not valid Lua: incomplete parsing must stay
	// visible even when the subsequent exact-name query has zero matches.
	index := run("source", "index", "--snapshot", sourcePin)["result"].(map[string]any)
	if index["complete"] != false || index["diagnostics"].(float64) < 1 {
		t.Fatalf("hidden syntax failure: %v", index)
	}
	query := run("source", "query", "NotDeclared", "--project", project, "--limit", "5")
	matches := query["result"].(map[string]any)
	if len(matches["matches"].([]any)) != 0 || matches["index"].(map[string]any)["complete"] != false {
		t.Fatalf("query hid incomplete index: %v", matches)
	}
	queryCapture := query["captures"].([]any)[0].(map[string]any)
	if queryCapture["complete"] != false {
		t.Fatalf("query falsely marked complete: %v", queryCapture)
	}
	run("evidence", "verify", queryCapture["id"].(string))

	prepareVersion := func(content string) (string, string) {
		t.Helper()
		object := git(content, "hash-object", "-w", "--stdin")
		tree := git("100644 blob "+object+"\tfixture.lua\n", "mktree")
		commit := git("comparison fixture", "commit-tree", tree)
		selectionJSON := `{"source":{"repository":"wow-ui-source","product":"retail","requestedRef":"fixture","exactCommit":"` + commit + `","parserRevision":"` + codebase.ParserRevision + `"}}`
		if err := os.WriteFile(spec, []byte(selectionJSON), 0600); err != nil {
			t.Fatal(err)
		}
		pin := run("target", "resolve", "--file", spec)["result"].(map[string]any)["id"].(string)
		run("source", "index", "--snapshot", pin)
		return pin, commit
	}
	beforePin, beforeCommit := prepareVersion("function Example(a)\nend\n")
	afterPin, afterCommit := prepareVersion("function Example(a,b)\nend\n")
	comparison := run("source", "diff", "--from", beforePin, "--to", afterPin, "--limit", "10")
	delta := comparison["result"].(map[string]any)
	if delta["documentChanges"] != float64(1) || delta["declarationChanges"] != float64(1) || delta["truncated"] != false {
		t.Fatalf("wrong source diff: %v", delta)
	}
	diffCapture := comparison["captures"].([]any)[0].(map[string]any)["id"].(string)
	run("evidence", "verify", diffCapture)
	diffRef, err := evidence.OpenArchive(store, metadata).InspectCapture(ctx, diffCapture)
	if err != nil || diffRef.Provenance.BaseSnapshot != beforePin || diffRef.Provenance.Snapshot != afterPin || diffRef.Provenance.BaseSourceCommit != beforeCommit || diffRef.Provenance.SourceCommit != afterCommit {
		t.Fatalf("diff lost an endpoint: %+v %v", diffRef, err)
	}
	addonRoot := filepath.Join(temp, "input-addon")
	if err := os.Mkdir(addonRoot, 0700); err != nil {
		t.Fatal(err)
	}
	manifestPath := filepath.Join(addonRoot, "Addon.toc")
	if err := os.WriteFile(manifestPath, []byte("## Interface: 120100\nmain.lua\n"), 0600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(addonRoot, "main.lua"), []byte("local value = 1\n"), 0600); err != nil {
		t.Fatal(err)
	}
	validation := run("source", "validate", "--snapshot", afterPin, "--path", addonRoot, "--toc", "Addon.toc")
	check := validation["result"].(map[string]any)
	if check["staticValid"] != true || check["complete"] != false || check["interfaceStatus"] != "unresolved" {
		t.Fatalf("invented coverage: %v", check)
	}
	if len(validation["warnings"].([]any)) == 0 {
		t.Fatal("incomplete coverage was silent")
	}
	if err := os.WriteFile(manifestPath, []byte("## Interface: 120100\nmissing.lua\n"), 0600); err != nil {
		t.Fatal(err)
	}
	failed := exec.Command(binary, "source", "validate", "--snapshot", afterPin, "--path", addonRoot, "--toc", "Addon.toc", "--home", root, "--format", "json")
	failedOutput, failedErr := failed.CombinedOutput()
	exit, ok := failedErr.(*exec.ExitError)
	if !ok || exit.ExitCode() != 4 {
		t.Fatalf("static failure exit: %v %s", failedErr, failedOutput)
	}
	var rejected map[string]any
	if err := json.Unmarshal(failedOutput, &rejected); err != nil {
		t.Fatal(err)
	}
	if rejected["ok"] != false || rejected["result"].(map[string]any)["staticValid"] != false || len(rejected["captures"].([]any)) != 1 {
		t.Fatalf("lost failed-check evidence: %v", rejected)
	}
	run("evidence", "verify", rejected["captures"].([]any)[0].(map[string]any)["id"].(string))
}
