package navigatetest

import (
	"bufio"
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/records"
	"github.com/follenfang/lycheedev/internal/vault"
)

func queryCSVFixture(t *testing.T, value int) (string, string, records.QueryReading) {
	t.Helper()
	fx := newFixture(t, []fixtureTable{{name: "QuerySeed", fileDataID: 201,
		columns: []fixtureColumn{{name: "ID", kind: 'i', bits: 32, identity: true}},
		rows:    [][]any{{uint64(1)}},
	}})
	reading, err := records.QueryData(context.Background(), fx.workspace, fx.pin.ID, fixtureQuery(), records.DataQuery{SQL: fmt.Sprintf("SELECT %d AS value FROM QuerySeed", value)})
	if err != nil {
		t.Fatal(err)
	}
	return fx.workspace, fx.pin.ID, reading
}

// Pause at the public exporter context boundary after CSV publication and
// before the manifest. This exercises the real two-file publication sequence
// without a test-only hook in production code.
type csvPublishPauseContext struct {
	context.Context
	output    string
	published chan struct{}
	resume    chan struct{}
	once      sync.Once
}

func (c *csvPublishPauseContext) Err() error {
	if raw, err := os.ReadFile(c.output); err == nil && string(raw) == "value\n1\n" {
		c.once.Do(func() {
			close(c.published)
			select {
			case <-c.resume:
			case <-c.Context.Done():
			}
		})
	}
	return c.Context.Err()
}

func TestQueryCSVExportSerializesDataAndManifestAcrossWorkspaces(t *testing.T) {
	rootA, snapshotA, queryA := queryCSVFixture(t, 1)
	rootB, snapshotB, _ := queryCSVFixture(t, 2)
	output := filepath.Join(t.TempDir(), "query.csv")
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()
	paused := &csvPublishPauseContext{Context: ctx, output: output, published: make(chan struct{}), resume: make(chan struct{})}
	var release sync.Once
	defer release.Do(func() { close(paused.resume) })
	type outcome struct {
		exported records.QueryCSVExport
		err      error
	}
	first := make(chan outcome, 1)
	go func() {
		exported, err := records.ExportQueryCSV(paused, rootA, snapshotA, queryA, output, true)
		first <- outcome{exported, err}
	}()
	select {
	case <-paused.published:
	case got := <-first:
		t.Fatalf("export did not reach CSV/manifest boundary: %+v", got)
	case <-ctx.Done():
		t.Fatal(ctx.Err())
	}

	// A different process and home use a textually different path to the same
	// destination. The first writer must hold the shared destination lease
	// until its completion manifest is published, not only around each file.
	alias := filepath.Dir(output) + string(filepath.Separator) + "unused" + string(filepath.Separator) + ".." + string(filepath.Separator) + filepath.Base(output)
	cmd := exec.CommandContext(ctx, os.Args[0], "-test.run=^TestQueryCSVExportHelperProcess$", "-test.v=false")
	cmd.Env = append(os.Environ(), "LYCHEEDEV_CSV_EXPORT_HELPER=1", "LYCHEEDEV_CSV_EXPORT_ROOT="+rootB, "LYCHEEDEV_CSV_EXPORT_SNAPSHOT="+snapshotB, "LYCHEEDEV_CSV_EXPORT_OUTPUT="+alias)
	stdout, err := cmd.StdoutPipe()
	if err != nil {
		t.Fatal(err)
	}
	stdin, err := cmd.StdinPipe()
	if err != nil {
		t.Fatal(err)
	}
	defer stdin.Close()
	var stderr bytes.Buffer
	cmd.Stderr = &stderr
	if err := cmd.Start(); err != nil {
		t.Fatal(err)
	}
	stopped := false
	defer func() {
		if !stopped {
			_ = cmd.Process.Kill()
			_ = cmd.Wait()
		}
	}()
	reader := bufio.NewReader(stdout)
	line, err := reader.ReadString('\n')
	if err != nil || strings.TrimSpace(line) != "blocked" {
		t.Fatalf("competing export did not wait for both publications: %q, %v; %s", line, err, stderr.String())
	}
	if raw, err := os.ReadFile(output); err != nil || string(raw) != "value\n1\n" {
		t.Fatalf("blocked export changed first CSV: %q, %v", raw, err)
	}
	if _, err := os.Stat(output + ".manifest.json"); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("blocked export published a manifest: %v", err)
	}
	release.Do(func() { close(paused.resume) })
	gotA := <-first
	if gotA.err != nil {
		t.Fatal(gotA.err)
	}
	checkQueryCSVPair(t, rootA, output, gotA.exported.Manifest)
	if _, err := fmt.Fprintln(stdin, "retry"); err != nil {
		t.Fatal(err)
	}
	line, err = reader.ReadString('\n')
	if err != nil {
		t.Fatal(err)
	}
	var manifestB records.QueryCSVManifest
	if err := json.Unmarshal([]byte(line), &manifestB); err != nil {
		t.Fatalf("second export result = %q: %v", line, err)
	}
	if err := cmd.Wait(); err != nil {
		stopped = true
		t.Fatalf("helper failed: %v; %s", err, stderr.String())
	}
	stopped = true
	checkQueryCSVPair(t, rootB, output, manifestB)
	if manifestB.QueryCapture == gotA.exported.Manifest.QueryCapture || manifestB.SHA256 == gotA.exported.Manifest.SHA256 {
		t.Fatalf("different queries did not replace both files: A=%+v B=%+v", gotA.exported.Manifest, manifestB)
	}
	if raw, err := os.ReadFile(output); err != nil || string(raw) != "value\n2\n" {
		t.Fatalf("final CSV = %q, %v", raw, err)
	}
}

func TestQueryCSVExportHelperProcess(t *testing.T) {
	if os.Getenv("LYCHEEDEV_CSV_EXPORT_HELPER") != "1" {
		return
	}
	root, snapshot, output := os.Getenv("LYCHEEDEV_CSV_EXPORT_ROOT"), os.Getenv("LYCHEEDEV_CSV_EXPORT_SNAPSHOT"), os.Getenv("LYCHEEDEV_CSV_EXPORT_OUTPUT")
	reading, err := records.QueryData(context.Background(), root, snapshot, fixtureQuery(), records.DataQuery{SQL: "SELECT 2 AS value FROM QuerySeed"})
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 300*time.Millisecond)
	_, err = records.ExportQueryCSV(ctx, root, snapshot, reading, output, true)
	cancel()
	if !errors.Is(err, context.DeadlineExceeded) {
		t.Fatalf("competing export returned %v, want deadline while first manifest is pending", err)
	}
	fmt.Fprintln(os.Stdout, "blocked")
	if _, err := bufio.NewReader(os.Stdin).ReadString('\n'); err != nil {
		t.Fatal(err)
	}
	result, err := records.ExportQueryCSV(context.Background(), root, snapshot, reading, output, true)
	if err != nil {
		t.Fatal(err)
	}
	if err := json.NewEncoder(os.Stdout).Encode(result.Manifest); err != nil {
		t.Fatal(err)
	}
}

func checkQueryCSVPair(t *testing.T, root, output string, want records.QueryCSVManifest) {
	t.Helper()
	raw, err := os.ReadFile(output)
	if err != nil {
		t.Fatal(err)
	}
	manifestRaw, err := os.ReadFile(output + ".manifest.json")
	if err != nil {
		t.Fatal(err)
	}
	var manifest records.QueryCSVManifest
	if err := json.Unmarshal(manifestRaw, &manifest); err != nil {
		t.Fatal(err)
	}
	digest := sha256.Sum256(raw)
	if manifest.SHA256 != hex.EncodeToString(digest[:]) || manifest.QueryCapture != want.QueryCapture || manifest.CSVCapture != want.CSVCapture || manifest.Bytes != len(raw) {
		t.Fatalf("CSV and completion record disagree: got %+v, want %+v", manifest, want)
	}
	_, err = vault.ReadWorkspace(context.Background(), root, func(s *vault.Store, m *vault.Metadata) (bool, error) {
		a := evidence.OpenArchive(s, m)
		ref, archived, err := a.FetchCapture(context.Background(), manifest.CSVCapture, 1<<20)
		if err != nil {
			return false, err
		}
		if !bytes.Equal(archived, raw) || ref.Provenance.Locator != manifest.QueryCapture {
			return false, errors.New("CSV capture does not match the published pair")
		}
		return true, a.VerifyCapture(context.Background(), manifest.QueryCapture)
	})
	if err != nil {
		t.Fatal(err)
	}
}
