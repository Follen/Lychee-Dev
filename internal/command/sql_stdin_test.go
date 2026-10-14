package command

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"os"
	"os/exec"
	"strings"
	"testing"
	"time"
)

func sqlStdinFixture(t *testing.T) (string, string) {
	t.Helper()
	return newDataFixture(t, []fixtureTable{{name: "TestNames", fileDataID: 91,
		columns: []fixtureColumn{{name: "ID", kind: 'i', bits: 32, identity: true}, {name: "Name", kind: 's'}},
		rows:    [][]any{{1, "First"}, {2, "Second"}},
	}})
}

func sqlStdinArgs(workspace, pin string) []string {
	return []string{"data", "sql", "--stdin", "--snapshot", pin, "--cdn", "--offline", "--home", workspace, "--format=json"}
}

type sqlStdinOutcome struct {
	code int
	out  bytes.Buffer
	log  bytes.Buffer
}

func checkSQLStdinOutcome(t *testing.T, result sqlStdinOutcome, wantCode int, wantError string) {
	t.Helper()
	var envelope Envelope
	if err := json.Unmarshal(result.out.Bytes(), &envelope); err != nil {
		t.Fatalf("decode result: %v: stdout=%s stderr=%s", err, result.out.String(), result.log.String())
	}
	if result.code != wantCode || envelope.OK != (wantCode == 0) {
		t.Fatalf("code=%d, want %d: %+v", result.code, wantCode, envelope)
	}
	if wantError != "" && (envelope.Error == nil || envelope.Error.Code != wantError) {
		t.Fatalf("error=%+v, want %s", envelope.Error, wantError)
	}
	if wantCode == 0 && (!strings.Contains(result.out.String(), `"rows":[[1],[2]]`) || len(envelope.Captures) != 1) {
		t.Fatalf("SQL did not execute through its pinned fixture: %s", result.out.String())
	}
	if result.log.Len() != 0 {
		t.Fatalf("unexpected stderr: %s", result.log.String())
	}
}

func TestSQLStdinExecute(t *testing.T) {
	workspace, pin := sqlStdinFixture(t)
	for _, mode := range []string{"deadline", "cancel", "eof"} {
		t.Run(mode, func(t *testing.T) {
			reader, writer, err := os.Pipe()
			if err != nil {
				t.Fatal(err)
			}
			defer reader.Close()
			defer writer.Close()
			previous := os.Stdin
			os.Stdin = reader
			defer func() { os.Stdin = previous }()
			ctx, cancel := context.WithCancel(context.Background())
			defer cancel()
			args := sqlStdinArgs(workspace, pin)
			wantCode, wantError := 0, ""
			switch mode {
			case "deadline":
				args = append(args, "--timeout-seconds", "1")
				wantCode, wantError = 5, "command.deadline"
			case "cancel":
				wantCode, wantError = 7, "command.cancelled"
			}
			finished := make(chan struct{})
			var result sqlStdinOutcome
			started := time.Now()
			go func() {
				defer close(finished)
				result.code = Execute(ctx, args, &result.out, &result.log)
			}()
			// Exceed the native pipe buffer so returning from Write proves the
			// reader consumed the SQL prefix. Any unread suffix is whitespace,
			// which is safe when stdin is reused below after cancellation.
			payload := "SELECT ID FROM TestNames"
			if mode != "eof" {
				payload += strings.Repeat(" ", 256<<10)
			}
			if _, err := io.WriteString(writer, payload); err != nil {
				t.Fatal(err)
			}
			if mode == "eof" {
				if err := writer.Close(); err != nil {
					t.Fatal(err)
				}
			} else if mode == "cancel" {
				// Give the query time to consume the SQL and block on its next
				// read; cancellation must work without closing the producer.
				time.Sleep(20 * time.Millisecond)
				cancel()
			}
			select {
			case <-finished:
			case <-time.After(3 * time.Second):
				writer.Close() // Unblock a regressed reader before reporting failure.
				<-finished
				t.Fatal("Execute remained blocked waiting for stdin EOF")
			}
			if mode == "deadline" && time.Since(started) < 900*time.Millisecond {
				t.Fatal("stdin returned before the requested query deadline")
			}
			checkSQLStdinOutcome(t, result, wantCode, wantError)
			if mode != "eof" {
				// The previous read and its cancellation must both be finished.
				// Reuse the still-open stdin immediately: no leaked reader may
				// consume this query, and no late cancellation may interrupt it.
				if _, err := io.WriteString(writer, "SELECT ID FROM TestNames"); err != nil {
					t.Fatalf("stdin was closed by cancellation: %v", err)
				}
				writer.Close()
				var next sqlStdinOutcome
				next.code = Execute(context.Background(), sqlStdinArgs(workspace, pin), &next.out, &next.log)
				checkSQLStdinOutcome(t, next, 0, "")
			}
		})
	}
}

// A new process gets real inherited os.Stdin, unlike assigning os.Pipe's reader
// in the parent. On Unix its descriptor starts blocking and has no Go deadline;
// Windows supplies its normal synchronous inherited pipe handle.
func TestSQLStdinInheritedProcess(t *testing.T) {
	if mode := os.Getenv("LYCHEEDEV_SQL_STDIN_TEST"); mode != "" {
		if err := os.Stdin.SetReadDeadline(time.Time{}); !errors.Is(err, os.ErrNoDeadline) {
			t.Fatalf("expected inherited stdin without Go deadlines, got %v", err)
		}
		ctx, cancel := context.WithCancel(context.Background())
		defer cancel()
		args := sqlStdinArgs(os.Getenv("LYCHEEDEV_SQL_STDIN_HOME"), os.Getenv("LYCHEEDEV_SQL_STDIN_PIN"))
		if mode == "deadline" {
			args = append(args, "--timeout-seconds", "1")
		} else if mode == "cancel" {
			timer := time.AfterFunc(200*time.Millisecond, cancel)
			defer timer.Stop()
		}
		os.Exit(Execute(ctx, args, os.Stdout, os.Stderr))
	}
	workspace, pin := sqlStdinFixture(t)
	executable, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	for _, mode := range []string{"deadline", "cancel", "eof"} {
		t.Run(mode, func(t *testing.T) {
			ctx, cancel := context.WithTimeout(context.Background(), 6*time.Second)
			defer cancel()
			cmd := exec.CommandContext(ctx, executable, "-test.run=^TestSQLStdinInheritedProcess$")
			cmd.Env = append(os.Environ(), "LYCHEEDEV_SQL_STDIN_TEST="+mode, "LYCHEEDEV_SQL_STDIN_HOME="+workspace, "LYCHEEDEV_SQL_STDIN_PIN="+pin)
			var result sqlStdinOutcome
			cmd.Stdout, cmd.Stderr = &result.out, &result.log
			writer, err := cmd.StdinPipe()
			if err != nil {
				t.Fatal(err)
			}
			defer writer.Close()
			if err := cmd.Start(); err != nil {
				t.Fatal(err)
			}
			if _, err := io.WriteString(writer, "SELECT ID FROM TestNames"); err != nil {
				cmd.Process.Kill()
				cmd.Wait()
				t.Fatal(err)
			}
			wantCode, wantError := 0, ""
			switch mode {
			case "deadline":
				wantCode, wantError = 5, "command.deadline"
			case "cancel":
				wantCode, wantError = 7, "command.cancelled"
			case "eof":
				writer.Close()
			}
			waitErr := cmd.Wait()
			if ctx.Err() != nil {
				t.Fatalf("inherited stdin outlived cancellation: %v, stdout=%s stderr=%s", ctx.Err(), result.out.String(), result.log.String())
			}
			var exitErr *exec.ExitError
			if errors.As(waitErr, &exitErr) {
				result.code = exitErr.ExitCode()
			} else if waitErr != nil {
				t.Fatal(waitErr)
			}
			checkSQLStdinOutcome(t, result, wantCode, wantError)
		})
	}
}

func TestSQLStdinRegularFile(t *testing.T) {
	workspace, pin := sqlStdinFixture(t)
	input, err := os.CreateTemp(t.TempDir(), "query-*.sql")
	if err != nil {
		t.Fatal(err)
	}
	defer input.Close()
	if _, err := io.WriteString(input, "SELECT ID FROM TestNames"); err != nil {
		t.Fatal(err)
	}
	if _, err := input.Seek(0, io.SeekStart); err != nil {
		t.Fatal(err)
	}
	previous := os.Stdin
	os.Stdin = input
	defer func() { os.Stdin = previous }()
	var result sqlStdinOutcome
	result.code = Execute(context.Background(), sqlStdinArgs(workspace, pin), &result.out, &result.log)
	checkSQLStdinOutcome(t, result, 0, "")
}
