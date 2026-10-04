//go:build windows && amd64

package process_test

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io/fs"
	"os"
	"os/exec"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/live/duplex"
	"github.com/follenfang/lycheedev/internal/live/duplexhost"
)

// This package smoke reads synthetic durable host evidence through the actual
// npm-installed entry point. It neither discovers nor writes a game process.
func TestInstalledDuplexReadOnlyRecovery(t *testing.T) {
	launcher := os.Getenv("LYCHEEDEV_DUPLEX_LAUNCHER")
	node := os.Getenv("LYCHEEDEV_DUPLEX_NODE")
	if launcher == "" || node == "" {
		t.Skip("installed-package fixture is invoked by install-smoke with an isolated launcher")
	}
	t.Logf("using installed duplex launcher: %s", launcher)
	root := t.TempDir()
	const connection = "CON-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
	dir := filepath.Join(root, ".lycheedev", "live", "duplex", connection)
	identity := duplex.Identity{Runtime: strings.Repeat("1", 32), Arena: strings.Repeat("2", 32), Session: strings.Repeat("3", 32), Owner: strings.Repeat("a", 32), ActorBinding: strings.Repeat("5", 32), Fence: 1}
	const request = "66666666666666666666666666666666"
	source := []byte("return 42")
	digest, err := duplex.RequestDigest(duplex.Header{RequestID: request, ActorBinding: identity.ActorBinding, BudgetMillis: 1000, CreatedUTCMillis: 1234, TotalBytes: uint32(len(source))}, source)
	if err != nil {
		t.Fatal(err)
	}
	body := []byte(`{"schema":"lycheedev.package.fixture.v1","value":42}`)
	bodyHash := sha256.Sum256(body)
	manifest := duplex.ResultManifest{RequestID: request, RequestSHA256: digest, State: "success", SHA256: hex.EncodeToString(bodyHash[:]), Bytes: uint32(len(body)), Pages: 1, PageSHA256: []string{hex.EncodeToString(bodyHash[:])}}
	store := duplex.NewFileStore(dir)
	ctx := context.Background()
	if err = store.SaveResult(ctx, manifest, body); err != nil {
		t.Fatal(err)
	}
	if err = store.Update(ctx, func(st *duplex.State) error {
		st.Identity, st.Bound, st.Closed, st.RequestSequence = identity, true, true, 1
		st.Active = &duplex.ActiveRequest{RequestID: request, Digest: digest, Sequence: 1, Attempt: 1, Created: 1234, Budget: 1000, TransferDeadline: 601234, ExecutionObservedHostAt: 1240, Phase: "released", Result: &manifest, ResultSaved: true, Released: true}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	workspaceHash := sha256.Sum256([]byte(strings.ToLower(root)))
	meta := map[string]any{"schema": "lycheedev.duplex.target.v1", "identity": identity, "claim": map[string]string{"operationId": connection, "workspaceId": hex.EncodeToString(workspaceHash[:])[:32]}, "target": map[string]any{}}
	writeJSON := func(path string, value any) {
		t.Helper()
		p, e := json.Marshal(value)
		if e != nil {
			t.Fatal(e)
		}
		if e = os.WriteFile(path, p, 0600); e != nil {
			t.Fatal(e)
		}
	}
	writeJSON(filepath.Join(dir, "target.json"), meta)
	snapshot := func() map[string][32]byte {
		t.Helper()
		files := map[string][32]byte{}
		e := filepath.WalkDir(root, func(path string, entry fs.DirEntry, err error) error {
			if err != nil || entry.IsDir() {
				return err
			}
			p, err := os.ReadFile(path)
			if err == nil {
				files[path] = sha256.Sum256(p)
			}
			return err
		})
		if e != nil {
			t.Fatal(e)
		}
		return files
	}
	inspect := func() *duplexhost.ProjectResult {
		t.Helper()
		before := snapshot()
		commandCtx, cancel := context.WithTimeout(ctx, 30*time.Second)
		defer cancel()
		cmd := exec.CommandContext(commandCtx, node, launcher, "live", "status", connection, "--project", root, "--format=json")
		out, err := cmd.CombinedOutput()
		var exit *exec.ExitError
		if !errors.As(err, &exit) {
			// There is deliberately no current process. A capability error is
			// expected; the durable report must survive it in the envelope.
			t.Fatalf("synthetic absent runtime returned unexpected status: %v %s", err, out)
		}
		var envelope struct {
			OK     bool                      `json:"ok"`
			Result *duplexhost.ProjectResult `json:"result"`
		}
		if e := json.Unmarshal(out, &envelope); e != nil {
			t.Fatalf("installed status: %v %s", err, out)
		}
		if envelope.OK || envelope.Result == nil {
			t.Fatalf("missing retained result on unavailable runtime: %s", out)
		}
		if !reflect.DeepEqual(before, snapshot()) {
			t.Fatal("read-only installed status mutated host evidence or created a lease")
		}
		return envelope.Result
	}
	result := inspect()
	var report struct {
		Schema string `json:"schema"`
		Value  int    `json:"value"`
	}
	if err = json.Unmarshal(result.Report, &report); err != nil {
		t.Fatal(err)
	}
	if result.ReportState != "verified" || result.Cleanup != "complete" || !result.Closed || !result.Complete || result.Identity != identity || result.Operation != request || report.Schema != "lycheedev.package.fixture.v1" || report.Value != 42 {
		t.Fatalf("saved result was lost when runtime disappeared: %+v", result)
	}
	if err = os.WriteFile(filepath.Join(dir, "result-"+request+".bin"), []byte(`{"value":99}`), 0600); err != nil {
		t.Fatal(err)
	}
	if result = inspect(); result.ReportState != "unavailable" || len(result.Report) != 0 {
		t.Fatal("installed status trusted altered result bytes")
	}
}
