package command

import (
	"bytes"
	"context"
	"io"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/selection"
)

type streamStartWriter struct {
	bytes.Buffer
	onBegin func()
}

func (w *streamStartWriter) Write(data []byte) (int, error) {
	n, err := w.Buffer.Write(data)
	if bytes.Contains(data, []byte(`"frame":"begin"`)) {
		w.onBegin()
	}
	return n, err
}

func TestDataStreamDistinguishesDeadlineFromCancellation(t *testing.T) {
	root, pin := dataCommandFixture(t)
	for _, deadline := range []bool{false, true} {
		t.Run(strconv.FormatBool(deadline), func(t *testing.T) {
			ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
			defer cancel()
			out := &streamStartWriter{onBegin: func() {
				if !deadline {
					cancel()
				}
				<-ctx.Done()
			}}
			args := append([]string{"data", "db2", "stream", "Map", "--limit", "10", "--format=jsonl"}, dataArgs(pin, root)...)
			code := Execute(ctx, args, out, io.Discard)
			want := 7
			fault := "records.cancelled"
			if deadline {
				want = 5
				fault = "records.deadline"
			}
			if code != want || !strings.Contains(out.String(), fault) || !strings.Contains(out.String(), `"frame":"error"`) || strings.Contains(out.String(), `"frame":"end"`) {
				t.Fatalf("exit=%d want=%d output=%s", code, want, out.String())
			}
		})
	}
}

func TestTargetResolveRejectsMalformedSelectionInput(t *testing.T) {
	root := initWorkspace(t)
	for _, data := range []string{`{`, `{"source":3}`, `{"unknown":true}`, `{} {}`, `{}`, `null`, `{"source":{}}`} {
		file := filepath.Join(t.TempDir(), "selection.json")
		if err := os.WriteFile(file, []byte(data), 0600); err != nil {
			t.Fatal(err)
		}
		response, code := invoke(t, "target", "resolve", "--file", file, "--home", root, "--format=json")
		if code != 4 || response.Error == nil || response.Error.Code != "selection.spec_format" {
			t.Errorf("input=%q exit=%d error=%+v", data, code, response.Error)
		}
	}
	file := filepath.Join(t.TempDir(), "oversized.json")
	if err := os.WriteFile(file, []byte(strings.Repeat(" ", 65537)), 0600); err != nil {
		t.Fatal(err)
	}
	response, code := invoke(t, "target", "resolve", "--file", file, "--home", root, "--format=json")
	if code != 3 || response.Error == nil || response.Error.Code != "selection.spec_limit" {
		t.Fatalf("oversized selection: exit=%d error=%+v", code, response.Error)
	}
}

func TestRaidbotsCLIHonorsInputByteBudget(t *testing.T) {
	root, _, file, raw := hotfixCommandFixture(t)
	for _, limit := range []int{len(raw) - 1, len(raw)} {
		response, code := invoke(t, raidbotsArgs(file, "--home", root, "--max-bytes", strconv.Itoa(limit), "--format=json")...)
		if limit < len(raw) {
			if code != 3 || response.Error == nil || response.Error.Code != "records.hotfix_limit" || response.Result != nil {
				t.Errorf("input exceeds selected byte budget: exit=%d error=%+v", code, response.Error)
			}
		} else if code != 0 || !response.OK {
			t.Errorf("input fits budget: exit=%d error=%+v", code, response.Error)
		}
	}
}

func TestSourceListHonorsExplicitProject(t *testing.T) {
	root := initWorkspace(t)
	fixture := filepath.Join("..", "..", "tests", "fixtures", "codebase", "sources", "valid-retail")
	indexed, err := indexGitFixture(t, context.Background(), root, "wow-ui-source", "retail", fixture)
	if err != nil {
		t.Fatal(err)
	}
	project := t.TempDir()
	for _, args := range [][]string{
		{"project", "init", "--path", project, "--product", "retail"},
		{"project", "lock", "--path", project, "--snapshot", indexed.Snapshot.ID, "--home", root},
	} {
		response, code := invoke(t, append(args, "--format=json")...)
		if code != 0 {
			t.Fatalf("prepare project: %d %+v", code, response.Error)
		}
	}
	t.Chdir(project)
	catalog, code := invoke(t, "source", "list", "--home", root, "--format=json")
	if _, ok := catalog.Result.([]any); code != 0 || !ok {
		t.Fatalf("unselected source list must retain the repository catalog: %d %+v", code, catalog)
	}
	selected, code := invoke(t, "source", "list", "--project", project, "--home", root, "--format=json")
	if code != 0 || selected.Context["snapshot"] != indexed.Snapshot.ID || selected.Context["project"] != project {
		t.Fatalf("explicit project ignored: %d %+v", code, selected)
	}
	if resultMap(t, selected)["ready"] != true {
		t.Fatalf("project source readiness missing: %+v", selected)
	}
}

func TestDoctorMirrorUsesExplicitProject(t *testing.T) {
	root := initWorkspace(t)
	current, selected := t.TempDir(), t.TempDir()
	for dir, product := range map[string]string{current: "classic", selected: "retail"} {
		if _, err := selection.InitializeProject(context.Background(), dir, product); err != nil {
			t.Fatal(err)
		}
	}
	t.Chdir(current)
	response, code := invoke(t, "doctor", "--project", selected, "--home", root, "--offline", "--format=json")
	if code != 0 {
		t.Fatalf("doctor: %d %+v", code, response.Error)
	}
	check, ok := doctorChecks(t, response)["codebase.source_mirror"].(map[string]any)
	if !ok || !strings.Contains(check["detail"].(string), "mirror for retail") {
		t.Fatalf("doctor used current directory instead of explicit project: %+v", check)
	}
}
