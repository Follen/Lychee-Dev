package command

import (
	"bufio"
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/records"
)

// Stop at the public cancellation boundary after CSV publication but before
// its completion manifest. This exercises Execute without a production hook.
type hotfixCSVPublishContext struct {
	context.Context
	output            string
	published, resume chan struct{}
	once              sync.Once
}

func (c *hotfixCSVPublishContext) Err() error {
	if _, err := os.Stat(c.output); err == nil {
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

func TestDataHotfixCSVSerializesAcrossProcessesAndHomes(t *testing.T) {
	aliases := []string{"normalized", "symlink"}
	if runtime.GOOS == "windows" {
		aliases = append(aliases, "case")
	}
	for _, kind := range aliases {
		t.Run(kind, func(t *testing.T) {
			rootA, _, sourceA, _ := hotfixCommandFixture(t)
			rootB, _, sourceB, _ := hotfixCommandFixture(t)
			parent := t.TempDir()
			output := filepath.Join(parent, "hotfix.csv")
			alias := parent + string(filepath.Separator) + "unused" + string(filepath.Separator) + ".." + string(filepath.Separator) + "hotfix.csv"
			if kind == "symlink" {
				link := filepath.Join(t.TempDir(), "alias")
				if err := os.Symlink(parent, link); err != nil {
					t.Skipf("directory symlink unavailable: %v", err)
				}
				alias = filepath.Join(link, "hotfix.csv")
			} else if kind == "case" {
				alias = filepath.Join(parent, "HOTFIX.CSV")
			}
			ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
			defer cancel()
			paused := &hotfixCSVPublishContext{Context: ctx, output: output, published: make(chan struct{}), resume: make(chan struct{})}
			var release sync.Once
			defer release.Do(func() { close(paused.resume) })
			type outcome struct {
				code     int
				out, log string
			}
			first := make(chan outcome, 1)
			go func() {
				var out, log bytes.Buffer
				code := Execute(paused, raidbotsArgs(sourceA, "--home", rootA, "--status", "0", "--encoding", "csv", "--output", output, "--format=json"), &out, &log)
				first <- outcome{code, out.String(), log.String()}
			}()
			select {
			case <-paused.published:
			case got := <-first:
				t.Fatalf("export never reached CSV/manifest boundary: %+v", got)
			case <-ctx.Done():
				t.Fatal(ctx.Err())
			}
			firstCSV, err := os.ReadFile(output)
			if err != nil {
				t.Fatal(err)
			}
			cmd := exec.CommandContext(ctx, os.Args[0], "-test.run=^TestHotfixCSVExportHelperProcess$", "-test.v=false")
			cmd.Env = append(os.Environ(), "LYCHEEDEV_HOTFIX_CSV_HELPER=1", "LYCHEEDEV_HOTFIX_CSV_HOME="+rootB, "LYCHEEDEV_HOTFIX_CSV_SOURCE="+sourceB, "LYCHEEDEV_HOTFIX_CSV_OUTPUT="+alias)
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
			signal := func(want string) {
				t.Helper()
				line, err := reader.ReadString('\n')
				if err != nil || strings.TrimSpace(line) != want {
					t.Fatalf("helper %q, %v; want %q; %s", line, err, want, stderr.String())
				}
			}
			signal("blocked")
			if raw, err := os.ReadFile(output); err != nil || !bytes.Equal(raw, firstCSV) {
				t.Fatalf("blocked exporter modified CSV: %v", err)
			}
			if _, err := os.Stat(output + ".manifest.json"); !os.IsNotExist(err) {
				t.Fatalf("blocked exporter published manifest: %v", err)
			}
			if _, err := fmt.Fprintln(stdin, "retry"); err != nil {
				t.Fatal(err)
			}
			signal("started")
			release.Do(func() { close(paused.resume) })
			gotA := <-first
			if gotA.code != 0 {
				t.Fatalf("first export: %+v", gotA)
			}
			var response Envelope
			if err := json.NewDecoder(reader).Decode(&response); err != nil || !response.OK {
				t.Fatalf("second response: %+v %v", response, err)
			}
			if err := cmd.Wait(); err != nil {
				stopped = true
				t.Fatalf("helper: %v %s", err, stderr.String())
			}
			stopped = true
			raw, err := os.ReadFile(output)
			if err != nil {
				t.Fatal(err)
			}
			if bytes.Equal(raw, firstCSV) {
				t.Fatal("fixture did not publish a distinct second result")
			}
			sidecar, err := os.ReadFile(output + ".manifest.json")
			if err != nil {
				t.Fatal(err)
			}
			var manifest records.CSVExportManifest
			if err := json.Unmarshal(sidecar, &manifest); err != nil {
				t.Fatal(err)
			}
			hash := sha256.Sum256(raw)
			if manifest.Content.SHA256 != hex.EncodeToString(hash[:]) || manifest.Content.Bytes != int64(len(raw)) {
				t.Fatalf("CSV/manifest mismatch: %+v", manifest)
			}
			gotJSON, err := json.Marshal(response.Result)
			if err != nil {
				t.Fatal(err)
			}
			var gotManifest records.CSVExportManifest
			if err := json.Unmarshal(gotJSON, &gotManifest); err != nil || gotManifest.Content != manifest.Content || gotManifest.RowCount != manifest.RowCount {
				t.Fatalf("returned manifest differs: %v", err)
			}
			for _, entry := range mustReadHotfixExportDir(t, parent) {
				if strings.HasPrefix(entry.Name(), ".lycheedev-export-") {
					t.Fatalf("stage leaked: %s", entry.Name())
				}
			}
		})
	}
}
func mustReadHotfixExportDir(t *testing.T, path string) []os.DirEntry {
	t.Helper()
	entries, err := os.ReadDir(path)
	if err != nil {
		t.Fatal(err)
	}
	return entries
}

type hotfixCSVStartedContext struct {
	context.Context
	once sync.Once
}

func (c *hotfixCSVStartedContext) Err() error {
	c.once.Do(func() { fmt.Fprintln(os.Stdout, "started") })
	return c.Context.Err()
}
func TestHotfixCSVExportHelperProcess(t *testing.T) {
	if os.Getenv("LYCHEEDEV_HOTFIX_CSV_HELPER") != "1" {
		return
	}
	args := raidbotsArgs(os.Getenv("LYCHEEDEV_HOTFIX_CSV_SOURCE"), "--home", os.Getenv("LYCHEEDEV_HOTFIX_CSV_HOME"), "--status", "1", "--encoding", "csv", "--output", os.Getenv("LYCHEEDEV_HOTFIX_CSV_OUTPUT"), "--format=json")
	ctx, cancel := context.WithTimeout(context.Background(), 500*time.Millisecond)
	var out, log bytes.Buffer
	code := Execute(ctx, args, &out, &log)
	cancel()
	var response Envelope
	if code != 5 || json.Unmarshal(out.Bytes(), &response) != nil || response.Error == nil || response.Error.Code != "command.deadline" {
		t.Fatalf("waiting export: %d %s %s", code, out.String(), log.String())
	}
	fmt.Fprintln(os.Stdout, "blocked")
	if _, err := bufio.NewReader(os.Stdin).ReadString('\n'); err != nil {
		t.Fatal(err)
	}
	out.Reset()
	log.Reset()
	if code := Execute(&hotfixCSVStartedContext{Context: context.Background()}, args, &out, &log); code != 0 {
		t.Fatalf("retry: %d %s %s", code, out.String(), log.String())
	}
	fmt.Fprint(os.Stdout, out.String())
}
