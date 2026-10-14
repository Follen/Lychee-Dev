//go:build windows

package command

import (
	"bufio"
	"context"
	"encoding/binary"
	"encoding/json"
	"errors"
	"fmt"
	"net"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/codebase"
	"github.com/follenfang/lycheedev/internal/luals"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
	"golang.org/x/sys/windows"
)

// The test executable can serve the real internal broker entry. The controlled
// listener changes scheduling only: all pipe bytes, workers and leases are real.
func TestMain(m *testing.M) {
	if len(os.Args) > 1 && os.Args[1] == "--internal-luals-broker" {
		listen := luals.ListenBrokerPipe
		if os.Getenv("LYCHEEDEV_TEST_BROKER_BARRIERS") == "1" {
			listen = func(endpoint, sddl string) (net.Listener, error) {
				listener, err := luals.ListenBrokerPipe(endpoint, sddl)
				if err != nil {
					return nil, err
				}
				fmt.Println("listening")
				return &retirementListener{Listener: listener, input: bufio.NewReader(os.Stdin)}, nil
			}
		}
		os.Exit(sourceBrokerMain(context.Background(), os.Args[1:], listen))
	}
	os.Exit(m.Run())
}

type retirementListener struct {
	net.Listener
	input    *bufio.Reader
	once     sync.Once
	retiring atomic.Bool
}

func (l *retirementListener) Accept() (net.Conn, error) {
	conn, err := l.Listener.Accept()
	if err != nil {
		return nil, err
	}
	return &retirementConn{Conn: conn, owner: l}, nil
}

func (l *retirementListener) Close() error {
	var err error
	l.once.Do(func() {
		err = l.Listener.Close()
		fmt.Println("listener-closed")
		_, _ = l.input.ReadByte()
	})
	return err
}

type retirementConn struct {
	net.Conn
	owner       *retirementListener
	request     []byte
	readRequest bool
}

func (c *retirementConn) Read(data []byte) (int, error) {
	n, err := c.Conn.Read(data)
	if !c.readRequest {
		c.request = append(c.request, data[:n]...)
		if len(c.request) >= 4 && len(c.request) == 4+int(binary.LittleEndian.Uint32(c.request[:4])) {
			c.readRequest = true
			var request brokerRequest
			if c.owner.retiring.Load() && json.Unmarshal(c.request[4:], &request) == nil {
				fmt.Println("retiring-request-" + request.Action)
			}
		}
	}
	return n, err
}

func (c *retirementConn) Write(data []byte) (int, error) {
	n, err := c.Conn.Write(data)
	var response brokerResponse
	if len(data) > 4 && int(binary.LittleEndian.Uint32(data[:4])) == len(data)-4 && json.Unmarshal(data[4:], &response) == nil && response.State == "closed" {
		c.owner.retiring.Store(true)
		fmt.Println("closed-reply-written")
		_, _ = c.owner.input.ReadByte()
	}
	return n, err
}

func brokerRetirementFixture(t *testing.T) (string, string, selection.SourcePin) {
	t.Helper()
	ctx := context.Background()
	home := filepath.Join(t.TempDir(), "home")
	store, err := vault.Initialize(ctx, home)
	if err != nil {
		t.Fatal(err)
	}
	source := filepath.Join(t.TempDir(), "source")
	docs := filepath.Join(source, "Interface", "AddOns", "Blizzard_APIDocumentationGenerated")
	if err = os.MkdirAll(docs, 0700); err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(filepath.Join(docs, "Fixture.lua"), []byte(`local API={Name="Fixture",Type="System",Functions={{Name="FixtureAPI",Type="Function",Returns={{Name="v",Type="number"}}}}};APIDocumentation:AddDocumentationTable(API)`), 0600); err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(filepath.Join(source, "probe.lua"), []byte("function First()\n return FixtureAPI()\nend\nfunction Second()\n return FixtureAPI()\nend\nfunction Third()\n return FixtureAPI()\nend\n"), 0600); err != nil {
		t.Fatal(err)
	}
	git := func(args ...string) string {
		t.Helper()
		cmd := exec.Command("git", args...)
		cmd.Dir = source
		cmd.Env = append(os.Environ(), "GIT_AUTHOR_NAME=Fixture", "GIT_AUTHOR_EMAIL=fixture@example.invalid", "GIT_COMMITTER_NAME=Fixture", "GIT_COMMITTER_EMAIL=fixture@example.invalid")
		raw, err := cmd.CombinedOutput()
		if err != nil {
			t.Fatalf("git %v: %v: %s", args, err, raw)
		}
		return strings.TrimSpace(string(raw))
	}
	git("init", "--quiet")
	git("add", ".")
	git("-c", "commit.gpgSign=false", "commit", "--quiet", "-m", "fixture")
	commit := git("rev-parse", "HEAD")
	git("clone", "--bare", "--quiet", source, filepath.Join(home, "mirrors", "wow-ui-source.git"))
	pin := selection.SourcePin{Repository: "wow-ui-source", Product: "retail", RequestedRef: "refs/heads/live", ExactCommit: commit, ParserRevision: codebase.ParserRevision}
	if _, err = codebase.OpenBrowser(store).IndexSource(ctx, pin); err != nil {
		t.Fatal(err)
	}
	set, err := vault.WriteMetadata(ctx, home, func(_ *vault.Store, metadata *vault.Metadata) (selection.PinnedSet, error) {
		return selection.OpenPinner(metadata).PinSelection(ctx, selection.SelectionSpec{Source: &pin})
	})
	if err != nil {
		t.Fatal(err)
	}
	return home, set.ID, pin
}

func TestRealBrokerCloseWaitsForProcessCleanup(t *testing.T) {
	release := os.Getenv("LYCHEEDEV_SOURCE_PACKAGE_RELEASE_ROOT")
	if release == "" {
		t.Skip("set an isolated verified release with the pinned LuaLS runtime")
	}
	for _, mode := range []string{"reply", "listener"} {
		t.Run(mode, func(t *testing.T) { testBrokerRetirementBarrier(t, release, mode) })
	}
}

func testBrokerRetirementBarrier(t *testing.T, release, mode string) {
	home, snapshot, pin := brokerRetirementFixture(t)
	ctx, cancel := context.WithTimeout(context.Background(), 45*time.Second)
	defer cancel()
	started, _, err := luals.BrokerProcessIdentity(uint32(os.Getpid()))
	if err != nil {
		t.Fatal(err)
	}
	token := strings.Repeat("a", 64)
	child := exec.CommandContext(ctx, os.Args[0], "--internal-luals-broker", home, release, strconv.Itoa(os.Getpid()), strconv.FormatUint(started, 10), token)
	child.Env = append(os.Environ(), "LYCHEEDEV_BROKER_START="+token, "LYCHEEDEV_TEST_BROKER_BARRIERS=1")
	child.Stderr = os.Stderr
	input, err := child.StdinPipe()
	if err != nil {
		t.Fatal(err)
	}
	output, err := child.StdoutPipe()
	if err != nil {
		t.Fatal(err)
	}
	if err = child.Start(); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { input.Close(); _ = child.Process.Kill(); _ = child.Wait() })
	events := make(chan string, 8)
	go func() {
		scanner := bufio.NewScanner(output)
		for scanner.Scan() {
			events <- scanner.Text()
		}
		close(events)
	}()
	wait := func(want string) {
		t.Helper()
		select {
		case got := <-events:
			if got != want {
				t.Fatalf("event=%q want=%q", got, want)
			}
		case <-ctx.Done():
			t.Fatal(ctx.Err())
		}
	}
	wait("listening")
	query := func(symbol string) (Envelope, int) {
		return invoke(t, "source", "refs", "--snapshot", snapshot, "--symbol", symbol, "--session-reuse", "--direction", "both", "--release", release, "--home", home, "--format=json")
	}
	first, code := query("First")
	if code != 0 || !first.OK {
		t.Fatalf("initial real query: %d %+v", code, first)
	}
	var ready Envelope
	if _, err = runSourceSession(ctx, "source session status", Options{home: home, release: release}, &ready); err != nil {
		t.Fatal(err)
	}
	workerPID := uint32(ready.Result.(map[string]any)["workerPid"].(int))
	worker, err := windows.OpenProcess(windows.SYNCHRONIZE, false, workerPID)
	if err != nil {
		t.Fatal(err)
	}
	defer windows.CloseHandle(worker)
	// Expiry is the test's release condition, not a probabilistic race window:
	// the server cannot finish its handler until this parent opens the barrier.
	closeCtx, stopClose := context.WithTimeout(ctx, time.Second)
	defer stopClose()
	closed := make(chan error, 1)
	go func() {
		if mode == "reply" {
			closed <- retireSourceBroker(closeCtx, home, release)
		} else {
			closed <- retireSourceBrokerIfPresent(closeCtx, home)
		}
	}()
	wait("closed-reply-written")
	if state, err := windows.WaitForSingleObject(worker, 0); err != nil || state != windows.WAIT_OBJECT_0 {
		t.Fatalf("runtime still alive at closed reply: state=%d err=%v", state, err)
	}
	worktree, err := vault.TryAcquireLease(ctx, filepath.Join(home, "locks"), "source:v1:worktree:"+pin.Repository+":"+pin.ExactCommit)
	if err != nil {
		t.Fatalf("worktree lease not released at closed reply: %v", err)
	}
	worktree.Close()
	if mode == "listener" {
		if _, err = input.Write([]byte{'r'}); err != nil {
			t.Fatal(err)
		}
		wait("listener-closed")
	}
	closeErr := <-closed
	if closeErr == nil {
		// Demonstrate the actual consequence before reporting the premature ack.
		// The real worker is parked after retire, so this fresh status request is
		// accepted by the old listener but cannot receive a worker response.
		if mode == "reply" {
			statusCtx, stopStatus := context.WithTimeout(ctx, 2*time.Second)
			var response Envelope
			_, statusErr := runSourceSession(statusCtx, "source session status", Options{home: home, release: release}, &response)
			stopStatus()
			wait("retiring-request-status")
			t.Errorf("close acknowledged while old listener is live; immediate status failed: %v", statusErr)
		} else {
			t.Error("close acknowledged before broker ownership cleanup")
		}
	} else if !errors.Is(closeErr, context.DeadlineExceeded) {
		t.Errorf("close deadline=%v", closeErr)
	}
	if mode == "reply" {
		if _, err = input.Write([]byte{'r'}); err != nil {
			t.Fatal(err)
		}
		wait("listener-closed")
	}
	_, _, scope, err := luals.BrokerScope(home)
	if err != nil {
		t.Fatal(err)
	}
	for _, lock := range [][2]string{{filepath.Join(home, "locks"), "source:v1:broker-owner"}, {scope, "source-broker-aggregate.v1"}} {
		lease, err := vault.TryAcquireLease(ctx, lock[0], lock[1])
		if err == nil {
			lease.Close()
			t.Fatal("lease released before controlled listener cleanup")
		}
		if !errors.Is(err, vault.ErrLeaseBusy) {
			t.Fatal(err)
		}
	}
	if _, err = input.Write([]byte{'l'}); err != nil {
		t.Fatal(err)
	}
	if err = child.Wait(); err != nil {
		t.Fatal(err)
	}
	var status Envelope
	if _, err = runSourceSession(ctx, "source session status", Options{home: home, release: release}, &status); err != nil || status.Result.(map[string]any)["state"] != "closed" {
		t.Fatalf("status after exit: %+v %v", status, err)
	}
	second, code := query("Second")
	if code != 0 || !second.OK {
		t.Fatalf("uncached query after reopening: %d %+v", code, second)
	}
	if err = retireSourceBroker(ctx, home, release); err != nil {
		t.Fatal(err)
	}
	// Successful close must also be a usable boundary, with no sleep or retry
	// between the acknowledgement, status, and a new uncached semantic request.
	if _, err = runSourceSession(ctx, "source session status", Options{home: home, release: release}, &status); err != nil || status.Result.(map[string]any)["state"] != "closed" {
		t.Fatalf("status immediately after successful close: %+v %v", status, err)
	}
	third, code := query("Third")
	if code != 0 || !third.OK {
		t.Fatalf("uncached query immediately after successful close: %d %+v", code, third)
	}
	if err = retireSourceBrokerIfPresent(ctx, home); err != nil {
		t.Fatal(err)
	}
}
