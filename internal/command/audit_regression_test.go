package command

import (
	"bytes"
	"context"
	"encoding/json"
	"github.com/follenfang/lycheedev/internal/live"
	"github.com/follenfang/lycheedev/internal/vault"
	"os"
	"path/filepath"
	"testing"
)

type streamRecordWriter struct {
	bytes.Buffer
	afterRecord func()
}

func (w *streamRecordWriter) Write(data []byte) (int, error) {
	n, err := w.Buffer.Write(data)
	var frame struct {
		Frame string `json:"frame"`
	}
	if json.Unmarshal(bytes.TrimSpace(data), &frame) == nil && frame.Frame == "record" && w.afterRecord != nil {
		callback := w.afterRecord
		w.afterRecord = nil
		callback()
	}
	return n, err
}

func TestDB2StreamFinalizationFailureReachesPublicJSONL(t *testing.T) {
	root, pin := newDataFixture(t, dataFixtureTables(t)[:1])
	var stdout streamRecordWriter
	stdout.afterRecord = func() {
		path := filepath.Join(root, "tmp")
		if err := os.Remove(path); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(path, []byte("blocked archive staging"), 0600); err != nil {
			t.Fatal(err)
		}
	}
	var stderr bytes.Buffer
	args := append([]string{"data", "db2", "stream", "Map", "--limit", "10", "--format=jsonl"}, dataArgs(pin, root)...)
	code := Execute(context.Background(), args, &stdout, &stderr)
	if code == 0 || stdout.afterRecord != nil {
		t.Fatalf("code=%d output=%s stderr=%s", code, stdout.String(), stderr.String())
	}
	frames := bytes.Split(bytes.TrimSpace(stdout.Bytes()), []byte("\n"))
	errors := 0
	for _, line := range frames {
		var frame struct {
			Frame string `json:"frame"`
		}
		if err := json.Unmarshal(line, &frame); err != nil {
			t.Fatal(err)
		}
		if frame.Frame == "end" {
			t.Fatal("failure emitted success end")
		}
		if frame.Frame == "error" {
			errors++
		}
	}
	if errors != 1 {
		t.Fatalf("error frames=%d output=%s", errors, stdout.String())
	}
}

func TestUpdateMissingReleasePreservesEnvironmentExit(t *testing.T) {
	r, c := invoke(t, "update", "--home", t.TempDir(), "--format=json")
	if c != 3 || r.OK {
		t.Fatalf("code=%d response=%+v", c, r)
	}
}
func TestProbeListPublicLimit(t *testing.T) {
	home := t.TempDir()
	if _, e := vault.InitializeWorkspace(context.Background(), home); e != nil {
		t.Fatal(e)
	}
	if _, e := live.PutProbe(context.Background(), home, "fixture", []byte("return 1")); e != nil {
		t.Fatal(e)
	}
	for _, limit := range []string{"200", "201", "1000"} {
		r, c := invoke(t, "live", "probe", "list", "--limit", limit, "--home", home, "--format=json")
		if c != 0 || !r.OK {
			t.Fatalf("limit %s code=%d response=%+v", limit, c, r.Error)
		}
	}
	_, c := invoke(t, "live", "probe", "list", "--limit", "1001", "--home", home, "--format=json")
	if c != 2 {
		t.Fatalf("code=%d", c)
	}
}

func TestRetiredLiveDispatchStopsBeforeWorkspaceAccess(t *testing.T) {
	home := filepath.Join(t.TempDir(), "absent")
	for _, args := range [][]string{
		{"live", "reset"}, {"live", "bind"}, {"live", "probe", "load"}, {"live", "run", "OP-fixture"}, {"live", "ack", "OP-fixture"}, {"live", "finish", "OP-fixture"}, {"live", "hide"},
		{"live", "resume", "OP-prepared"}, {"live", "resume", "OP-submitted"}, {"live", "resume", "BTP-uncertain"},
	} {
		r, c := invoke(t, append(args, "--home", home, "--format=json")...)
		if c != 2 || r.OK {
			t.Fatalf("%v: %d %+v", args, c, r)
		}
	}
	if _, err := os.Stat(home); !os.IsNotExist(err) {
		t.Fatalf("retired execution touched workspace: %v", err)
	}
}
