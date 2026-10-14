package command

import (
	"encoding/json"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/codebase"
	"github.com/follenfang/lycheedev/internal/selection"
)

// Build real Git objects without an index or network. The public target/diff
// commands then exercise the same fixed-pin preparation used after source sync.
func sourceDiffFixture(t *testing.T, before, after string) (string, selection.SourcePin, selection.SourcePin) {
	t.Helper()
	home := filepath.Join(t.TempDir(), "home")
	if response, code := invoke(t, "init", "--home", home, "--format=json"); code != 0 {
		t.Fatalf("initialize: %d %+v", code, response)
	}
	mirror := filepath.Join(home, "mirrors", "wow-ui-source.git")
	run := func(input string, args ...string) string {
		t.Helper()
		command := exec.Command("git", append([]string{"-c", "core.hooksPath=" + os.DevNull, "--git-dir=" + mirror}, args...)...)
		for _, entry := range os.Environ() {
			key, _, _ := strings.Cut(entry, "=")
			if !strings.HasPrefix(strings.ToUpper(key), "GIT_") {
				command.Env = append(command.Env, entry)
			}
		}
		command.Env = append(command.Env,
			"GIT_CONFIG_NOSYSTEM=1", "GIT_CONFIG_GLOBAL="+os.DevNull,
			"GIT_AUTHOR_NAME=SourceDiffFixture", "GIT_AUTHOR_EMAIL=fixture@example.invalid",
			"GIT_COMMITTER_NAME=SourceDiffFixture", "GIT_COMMITTER_EMAIL=fixture@example.invalid",
			"GIT_AUTHOR_DATE=2026-01-01T00:00:00Z", "GIT_COMMITTER_DATE=2026-01-01T00:00:00Z")
		command.Stdin = strings.NewReader(input)
		out, err := command.CombinedOutput()
		if err != nil {
			t.Fatalf("git %v: %v: %s", args, err, out)
		}
		return strings.TrimSpace(string(out))
	}
	run("", "init", "--bare", "--object-format=sha1", mirror)
	commit := func(content, message string) selection.SourcePin {
		blob := run(content, "hash-object", "-w", "--stdin")
		tree := run("100644 blob "+blob+"\tfixture.lua\n", "mktree")
		id := run(message+"\n", "commit-tree", tree)
		return selection.SourcePin{Repository: "wow-ui-source", Product: "retail", RequestedRef: id, ExactCommit: id, ParserRevision: codebase.ParserRevision}
	}
	left, right := commit(before, "before"), commit(after, "after")
	// A moving branch is deliberately unrelated to either requested revision.
	latest := commit("function UnrequestedLatest() end\n", "unrequested")
	run("", "update-ref", "refs/heads/live", latest.ExactCommit)
	return home, left, right
}

func sourceDiffPin(t *testing.T, home string, source selection.SourcePin) string {
	t.Helper()
	raw, err := json.Marshal(selection.SelectionSpec{Source: &source})
	if err != nil {
		t.Fatal(err)
	}
	file := filepath.Join(t.TempDir(), "selection.json")
	if err := os.WriteFile(file, raw, 0600); err != nil {
		t.Fatal(err)
	}
	response, code := invoke(t, "target", "resolve", "--file", file, "--home", home, "--format=json")
	if code != 0 || !response.OK {
		t.Fatalf("pin: %d %+v", code, response)
	}
	return resultMap(t, response)["id"].(string)
}

func TestSourceDiffPreparesFixedSnapshots(t *testing.T) {
	for _, warmLeft := range []bool{false, true} {
		name := "cold_to_cold"
		if warmLeft {
			name = "warm_to_cold"
		}
		t.Run(name, func(t *testing.T) {
			home, left, right := sourceDiffFixture(t,
				"function Changed(value)\nreturn value\nend\n",
				"function Changed(other)\nreturn other\nend\nfunction Added() end\n")
			from, to := sourceDiffPin(t, home, left), sourceDiffPin(t, home, right)
			if warmLeft {
				if response, code := invoke(t, "source", "index", "--snapshot", from, "--home", home, "--format=json"); code != 0 {
					t.Fatalf("warm left: %d %+v", code, response)
				}
			}
			args := []string{"source", "diff", "--from", from, "--to", to, "--limit", "50", "--home", home, "--format=json"}
			for attempt := 0; attempt < 2; attempt++ {
				response, code := invoke(t, args...)
				if code != 0 || !response.OK || len(response.Captures) != 1 || response.Context["fromSnapshot"] != from || response.Context["toSnapshot"] != to {
					t.Fatalf("diff attempt %d: %d %+v", attempt, code, response)
				}
				result := resultMap(t, response)
				if result["from"].(map[string]any)["commit"] != left.ExactCommit || result["to"].(map[string]any)["commit"] != right.ExactCommit || result["documentChanges"] != float64(1) || result["declarationChanges"] != float64(2) || result["truncated"] != false {
					t.Fatalf("fixed comparison changed: %+v", result)
				}
				capture := response.Captures[0].(map[string]any)
				provenance := capture["provenance"].(map[string]any)
				if provenance["snapshot"] != to || provenance["sourceCommit"] != right.ExactCommit || provenance["baseSnapshot"] != from || provenance["baseSourceCommit"] != left.ExactCommit || capture["complete"] != true {
					t.Fatalf("comparison provenance changed: %+v", capture)
				}
				if checked, exit := invoke(t, "evidence", "verify", capture["id"].(string), "--home", home, "--format=json"); exit != 0 || !checked.OK {
					t.Fatalf("verify diff: %d %+v", exit, checked)
				}
			}
		})
	}
}

func TestSourceDiffPreparationKeepsRejections(t *testing.T) {
	for _, condition := range []string{"incomplete", "repository", "parser"} {
		t.Run(condition, func(t *testing.T) {
			after := "function Changed(other) end\n"
			if condition == "incomplete" {
				after = "function Broken(\n"
			}
			home, left, right := sourceDiffFixture(t, "function Changed(value) end\n", after)
			if condition == "repository" {
				right.Repository, right.Product = "elvui", "main"
			}
			if condition == "parser" {
				right.ParserRevision = "another-parser"
			}
			from, to := sourceDiffPin(t, home, left), sourceDiffPin(t, home, right)
			response, code := invoke(t, "source", "diff", "--from", from, "--to", to, "--home", home, "--format=json")
			want := "codebase.incompatible_comparison"
			if condition == "incomplete" {
				want = "codebase.incomplete_index"
			}
			if code == 0 || response.OK || response.Error == nil || !strings.Contains(response.Error.Message, want) || response.Result != nil || len(response.Captures) != 0 {
				t.Fatalf("%s comparison was not rejected: %d %+v", condition, code, response)
			}
			if condition != "incomplete" {
				entries, err := os.ReadDir(filepath.Join(home, "source", "v1", "facts", "snapshots"))
				if err != nil && !os.IsNotExist(err) || len(entries) != 0 {
					t.Fatalf("incompatible pair prepared mappings: %v %v", entries, err)
				}
			}
		})
	}
}
