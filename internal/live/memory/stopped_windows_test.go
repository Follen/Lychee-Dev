//go:build windows && amd64

package memory

import (
	"bytes"
	"context"
	"encoding/binary"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"os/exec"
	"syscall"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/live/duplex"
	"golang.org/x/sys/windows"
)

const ownedStopTargetArg = "--test-owned-mailbox-stop-target"
const ownedStopWriterArg = "--test-owned-mailbox-stop-writer"
const ownedStopPeerArg = "--test-owned-mailbox-stop-peer"

// These roles exist only in the test executable. They expose owned allocation
// addresses to its parent, never to the production helper or game CLI.
func TestMain(m *testing.M) {
	if handled, code := RunMailboxMemoryHelper(os.Args[1:], os.Stdin, os.Stdout); handled {
		os.Exit(code)
	}
	if len(os.Args) == 2 {
		if os.Args[1] == ownedStopTargetArg {
			os.Exit(runOwnedStopTarget())
		}
		if os.Args[1] == ownedStopWriterArg {
			os.Exit(runOwnedStopWriter())
		}
		if os.Args[1] == ownedStopPeerArg {
			parent, e := stoppedInputParent(os.Stdin)
			if e != nil {
				fmt.Fprintln(os.Stderr, e)
				os.Exit(2)
			}
			json.NewEncoder(os.Stdout).Encode(parent)
			os.Exit(0)
		}
	}
	os.Exit(m.Run())
}

type ownedStopTarget struct {
	Identity ProcessIdentity
	Row      uint64
	Counters uint64
}
type ownedStopRequest struct {
	Target  ownedStopTarget
	Mode    string
	Message duplex.Message
}
type ownedStopResult struct {
	Stop    StoppedObservation
	Outcome duplex.WriteOutcome
	Error   string
}

func runOwnedStopTarget() int {
	at, e := windows.VirtualAlloc(0, duplexCommandWords*24+4096, windows.MEM_COMMIT|windows.MEM_RESERVE, windows.PAGE_READWRITE)
	if e != nil {
		return 2
	}
	row := numberRow(duplexCommandWords).image
	var n uintptr
	if e = windows.WriteProcessMemory(windows.CurrentProcess(), at, &row[0], uintptr(len(row)), &n); e != nil || int(n) != len(row) {
		return 3
	}
	id, e := processIdentity(windows.GetCurrentProcessId())
	if e != nil {
		return 4
	}
	d := ownedStopTarget{Identity: id, Row: uint64(at), Counters: uint64(at) + uint64(len(row))}
	if e = json.NewEncoder(os.Stdout).Encode(d); e != nil {
		return 5
	}
	done := make(chan struct{})
	go func() { io.Copy(io.Discard, os.Stdin); close(done) }()
	for worker := 0; worker < 3; worker++ {
		go func(i int) {
			tick := time.NewTicker(time.Millisecond)
			defer tick.Stop()
			var count uint64
			for {
				select {
				case <-done:
					return
				case <-tick.C:
					count++
					var raw [8]byte
					binary.LittleEndian.PutUint64(raw[:], count)
					var transferred uintptr
					_ = windows.WriteProcessMemory(windows.CurrentProcess(), uintptr(d.Counters)+uintptr(i*8), &raw[0], 8, &transferred)
				}
			}
		}(worker)
	}
	<-done
	return 0
}

func runOwnedStopWriter() int {
	decoder := json.NewDecoder(os.Stdin)
	var request ownedStopRequest
	if e := decoder.Decode(&request); e != nil {
		return 2
	}
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	sentinel, cleanup, e := startStoppedSentinel(ctx)
	if e != nil {
		fmt.Fprintln(os.Stderr, e)
		return 3
	}
	defer func() {
		if request.Mode == "detach_cleanup_block" {
			_ = json.NewEncoder(os.Stdout).Encode("detach_failed")
			select {} // Simulate a blocked post-detach sentinel cleanup.
		}
		cleanup()
	}()
	var result ownedStopResult
	stopReturned := false
	result.Outcome.State = duplex.NoWrite
	detachFailure := request.Mode == "detach_cleanup_block" || request.Mode == "detach_output_block"
	if request.Mode == "watchdog" || detachFailure {
		watchCtx, watchCancel := context.WithTimeout(ctx, 150*time.Millisecond)
		disarm := stoppedWatchdog(watchCtx)
		defer func() { finishStoppedWatchdog(stopReturned, result.Stop, disarm, watchCancel) }()
	}
	if request.Mode == "identity_created" {
		request.Target.Identity.Created++
	}
	if request.Mode == "identity_image" {
		request.Target.Identity.Image += ".wrong"
	}
	detach := func(pid uint32) error {
		if detachFailure && pid == request.Target.Identity.PID {
			return errors.New("injected target detach failure")
		}
		return debugCall(debugDetach, uintptr(pid))
	}
	result.Stop, e = withStoppedProcessDetach(ctx, request.Target.Identity, sentinel, func(s *stoppedProcess) error {
		if e := s.verify(ctx); e != nil {
			return e
		}
		if e := json.NewEncoder(os.Stdout).Encode("frozen"); e != nil {
			return e
		}
		// Parent controls this block to inspect heartbeat or kill the helper.
		// Production instead has a fixed-duration process termination watchdog.
		var proceed string
		if e := decoder.Decode(&proceed); e != nil {
			return e
		}
		if proceed != "go" {
			return errors.New("invalid owned test release")
		}
		if request.Mode == "cancel" {
			cancel()
			return s.verify(ctx)
		}
		if request.Mode == "noop" || detachFailure {
			return s.verify(ctx)
		}
		old := make([]byte, duplexCommandWords*24)
		if n, e := s.process.Read(ctx, request.Target.Row, old); e != nil || n != len(old) {
			return errors.Join(io.ErrUnexpectedEOF, e)
		}
		plan, e := planDuplexRow(request.Message, &duplexRow{image: old, frozen: true})
		if e != nil {
			return e
		}
		write := func(b []byte) (int, error) {
			var transferred uintptr
			if request.Mode == "partial" {
				// Deliberately write a prefix only to this owned fixture.
				e := windows.WriteProcessMemory(s.process.handle, uintptr(request.Target.Row), &b[0], 24, &transferred)
				return int(transferred), errors.Join(e, io.ErrShortWrite)
			}
			e := windows.WriteProcessMemory(s.process.handle, uintptr(request.Target.Row), &b[0], uintptr(len(b)), &transferred)
			return int(transferred), e
		}
		var fact DuplexWriteRange
		result.Outcome, fact, e = executeDuplexRow(ctx, plan, s.verify, write, func(c context.Context, b []byte) (int, error) { return s.process.Read(c, request.Target.Row, b) }, s.verify)
		_ = fact
		if request.Mode == "crash_after_write" && e == nil {
			if e = json.NewEncoder(os.Stdout).Encode("written"); e != nil {
				return e
			}
			// Parent kills us without receiving a publication outcome.
			var end string
			return decoder.Decode(&end)
		}
		return e
	}, detach)
	stopReturned = true
	if e != nil {
		result.Error = e.Error()
	}
	if detachFailure {
		if !result.Stop.Attached || result.Stop.Detached {
			return 5
		}
		if request.Mode == "detach_output_block" {
			_ = json.NewEncoder(os.Stdout).Encode("detach_failed")
			select {} // Simulate a blocked response writer while still attached.
		}
		return 0 // Runs conditional watchdog completion, then blocked cleanup.
	}
	if e = json.NewEncoder(os.Stdout).Encode(result); e != nil {
		return 4
	}
	return 0
}

func newOwnedStopTarget(t *testing.T) (ownedStopTarget, *Process) {
	t.Helper()
	exe, e := os.Executable()
	if e != nil {
		t.Fatal(e)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
	cmd := exec.CommandContext(ctx, exe, ownedStopTargetArg)
	cmd.SysProcAttr = &syscall.SysProcAttr{CreationFlags: windows.CREATE_NO_WINDOW}
	stdin, e := cmd.StdinPipe()
	if e != nil {
		t.Fatal(e)
	}
	stdout, e := cmd.StdoutPipe()
	if e != nil {
		t.Fatal(e)
	}
	var stderr bytes.Buffer
	cmd.Stderr = &stderr
	if e = cmd.Start(); e != nil {
		t.Fatal(e)
	}
	t.Cleanup(func() { stdin.Close(); _ = cmd.Process.Kill(); _ = cmd.Wait(); cancel() })
	var d ownedStopTarget
	if e = json.NewDecoder(stdout).Decode(&d); e != nil {
		t.Fatalf("owned target: %v %s", e, stderr.String())
	}
	p, e := Open(d.Identity.PID, d.Identity.Created, d.Identity.Image)
	if e != nil {
		t.Fatal(e)
	}
	t.Cleanup(func() { p.Close() })
	return d, p
}

func ownedHeartbeat(t *testing.T, p *Process, d ownedStopTarget) [24]byte {
	t.Helper()
	var b [24]byte
	if n, e := p.Read(context.Background(), d.Counters, b[:]); e != nil || n != len(b) {
		t.Fatal(n, e)
	}
	return b
}

func assertOwnedResumed(t *testing.T, p *Process, d ownedStopTarget, before [24]byte) {
	t.Helper()
	deadline := time.Now().Add(time.Second)
	for time.Now().Before(deadline) {
		b := ownedHeartbeat(t, p, d)
		all := true
		for i := 0; i < 3; i++ {
			if binary.LittleEndian.Uint64(b[i*8:]) <= binary.LittleEndian.Uint64(before[i*8:]) {
				all = false
			}
		}
		if all {
			return
		}
		time.Sleep(5 * time.Millisecond)
	}
	t.Fatal("owned target heartbeat failed to resume on all worker threads")
}

func TestStoppedOwnedProcessResumeCancelPartialAndCrash(t *testing.T) {
	for _, mode := range []string{"noop", "cancel", "write", "partial", "crash", "watchdog", "crash_after_write", "identity_created", "identity_image", "detach_cleanup_block", "detach_output_block"} {
		t.Run(mode, func(t *testing.T) {
			d, p := newOwnedStopTarget(t)
			time.Sleep(10 * time.Millisecond)
			exe, _ := os.Executable()
			ctx, cancel := context.WithTimeout(context.Background(), 8*time.Second)
			defer cancel()
			cmd := exec.CommandContext(ctx, exe, ownedStopWriterArg)
			cmd.SysProcAttr = &syscall.SysProcAttr{CreationFlags: windows.CREATE_NO_WINDOW}
			stdin, e := cmd.StdinPipe()
			if e != nil {
				t.Fatal(e)
			}
			stdout, e := cmd.StdoutPipe()
			if e != nil {
				t.Fatal(e)
			}
			var stderr bytes.Buffer
			cmd.Stderr = &stderr
			if e = cmd.Start(); e != nil {
				t.Fatal(e)
			}
			waited := false
			t.Cleanup(func() {
				stdin.Close()
				if !waited {
					_ = cmd.Process.Kill()
					_ = cmd.Wait()
				}
			})
			message := commandRowMessage(t, bytes.Repeat([]byte{0x42}, duplex.MaxSourceBytes))
			if e = json.NewEncoder(stdin).Encode(ownedStopRequest{Target: d, Mode: mode, Message: message}); e != nil {
				t.Fatal(e)
			}
			decoder := json.NewDecoder(stdout)
			if mode == "identity_created" || mode == "identity_image" {
				before := ownedHeartbeat(t, p, d)
				var result ownedStopResult
				if e = decoder.Decode(&result); e != nil {
					t.Fatalf("identity failure result: %v %s", e, stderr.String())
				}
				if e = cmd.Wait(); e != nil {
					t.Fatal(e)
				}
				waited = true
				if result.Error == "" || result.Outcome.State != duplex.NoWrite || result.Stop.DebuggerThread != 0 {
					t.Fatal(result)
				}
				assertOwnedResumed(t, p, d, before)
				return
			}
			var marker string
			if e = decoder.Decode(&marker); e != nil || marker != "frozen" {
				t.Fatalf("freeze failed: %v stderr=%s", e, stderr.String())
			}
			before := ownedHeartbeat(t, p, d)
			time.Sleep(25 * time.Millisecond)
			if after := ownedHeartbeat(t, p, d); before != after {
				t.Fatal("threads ran while target CREATE_PROCESS event held")
			}
			if mode == "crash" || mode == "watchdog" {
				if mode == "crash" {
					if e = cmd.Process.Kill(); e != nil {
						t.Fatal(e)
					}
				}
				if e = cmd.Wait(); e == nil {
					t.Fatal("crashed helper unexpectedly succeeded")
				}
				waited = true
				assertOwnedResumed(t, p, d, before)
				return
			}
			if e = json.NewEncoder(stdin).Encode("go"); e != nil {
				t.Fatal(e)
			}
			if mode == "detach_cleanup_block" || mode == "detach_output_block" {
				var failed string
				if e = decoder.Decode(&failed); e != nil || failed != "detach_failed" {
					t.Fatal(failed, e)
				}
				start := time.Now()
				if e = cmd.Wait(); e == nil {
					t.Fatal("detach-failed helper escaped watchdog")
				}
				waited = true
				if elapsed := time.Since(start); elapsed > time.Second {
					t.Fatalf("detach-failure watchdog canceled prematurely: %s", elapsed)
				}
				assertOwnedResumed(t, p, d, before)
				return
			}
			if mode == "crash_after_write" {
				var written string
				if e = decoder.Decode(&written); e != nil || written != "written" {
					t.Fatal(written, e)
				}
				if e = cmd.Process.Kill(); e != nil {
					t.Fatal(e)
				}
				if e = cmd.Wait(); e == nil {
					t.Fatal("crashed helper succeeded")
				}
				waited = true
				assertOwnedResumed(t, p, d, before)
				actual := make([]byte, 24)
				if _, e = p.Read(ctx, d.Row, actual); e != nil {
					t.Fatal(e)
				}
				expected, e := planDuplexRow(message, numberRow(duplexCommandWords))
				if e != nil || !bytes.Equal(actual, expected.image[:24]) {
					t.Fatal("write lost despite unknown host outcome", e)
				}
				return
			}
			var result ownedStopResult
			if e = decoder.Decode(&result); e != nil {
				t.Fatalf("result: %v %s", e, stderr.String())
			}
			if e = cmd.Wait(); e != nil {
				t.Fatalf("helper: %v %s", e, stderr.String())
			}
			waited = true
			if !result.Stop.Detached || result.Stop.DebuggerThread == 0 {
				t.Fatalf("detach/thread proof: %+v", result)
			}
			switch mode {
			case "cancel":
				if result.Outcome.State != duplex.NoWrite || result.Error == "" {
					t.Fatal(result)
				}
			case "partial":
				if result.Outcome.State != duplex.PartialWrite || result.Outcome.Bytes != 24 || result.Error == "" {
					t.Fatal(result)
				}
			case "write":
				if result.Outcome.State != duplex.CompleteWrite || !result.Outcome.ReadbackVerified || result.Outcome.Bytes != 6293376 || result.Error != "" {
					t.Fatal(result)
				}
				expected, e := planDuplexRow(message, numberRow(duplexCommandWords))
				if e != nil {
					t.Fatal(e)
				}
				actual := make([]byte, len(expected.image))
				if _, e = p.Read(ctx, d.Row, actual); e != nil || !bytes.Equal(actual, expected.image) {
					t.Fatal("owned full row mismatch", e)
				}
			default:
				if result.Error != "" {
					t.Fatal(result)
				}
			}
			assertOwnedResumed(t, p, d, before)
			t.Logf("owned fixture attach=%dus stopped=%dus (includes deliberate 25ms hold), OS thread=%d", result.Stop.AttachMicros, result.Stop.StoppedMicros, result.Stop.DebuggerThread)
		})
	}
}

func TestStoppedPublisherRejectsUnqualifiedProfileWithoutAttachment(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	out, _, stop, e := PublishStoppedDuplexRow(ctx, StoppedPublicationRequest{Target: ProcessIdentity{PID: windows.GetCurrentProcessId()}, ExecutableSHA256: RetailLuaMailboxExecutableSHA256, Build: retailReloadSourceBuild, Product: "retail"})
	if e == nil || out.State != duplex.NoWrite || stop.DebuggerThread != 0 {
		t.Fatal(out, stop, e)
	}
}

func TestStoppedPublisherDeadlineAndHelperInputGuards(t *testing.T) {
	request := StoppedPublicationRequest{ExecutableSHA256: RetailLuaMailboxExecutableSHA256, Build: retailReloadSourceBuild, Product: "retail"}
	for _, ctx := range []context.Context{context.Background(), func() context.Context { c, cancel := context.WithCancel(context.Background()); cancel(); return c }()} {
		out, _, stop, e := PublishStoppedDuplexRow(ctx, request)
		if e == nil || out.State != duplex.NoWrite || stop.DebuggerThread != 0 {
			t.Fatal(out, stop, e)
		}
	}
	for _, input := range []string{`{"unexpected":true}`, `{} {}`, string(bytes.Repeat([]byte{'x'}, stoppedRequestLimit+1))} {
		r := runStoppedPublication(bytes.NewBufferString(input))
		if r.Error == "" || r.Outcome.State != duplex.NoWrite || r.Stop.DebuggerThread != 0 {
			t.Fatal(r)
		}
	}
	var out bytes.Buffer
	if handled, _ := RunMailboxMemoryHelper([]string{"--arbitrary-debug"}, nil, &out); handled {
		t.Fatal("unexpected helper role")
	}
	if handled, _ := RunMailboxMemoryHelper([]string{stoppedPublisherArg, "extra"}, nil, &out); handled {
		t.Fatal("unexpected helper arguments")
	}
}

func TestStoppedWatchdogCompletionRequiresConfirmedDetach(t *testing.T) {
	for _, stop := range []StoppedObservation{{}, {Attached: true, Detached: true}, {Attached: true, Detached: false}} {
		disarmed, canceled := false, false
		finishStoppedWatchdog(true, stop, func() { disarmed = true }, func() { canceled = true })
		want := !stop.Attached || stop.Detached
		if disarmed != want || canceled != want {
			t.Fatal(stop, disarmed, canceled)
		}
	}
	finishStoppedWatchdog(false, StoppedObservation{}, func() { t.Fatal("unreturned stop disarmed watchdog") }, func() { t.Fatal("unreturned stop canceled deadline") })
}

func TestStoppedHelperInputBoundToParentExecutable(t *testing.T) {
	exe, e := os.Executable()
	if e != nil {
		t.Fatal(e)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	cmd := exec.CommandContext(ctx, exe, ownedStopPeerArg)
	cmd.Stdin = bytes.NewReader([]byte("owned anonymous pipe"))
	var stdout, stderr bytes.Buffer
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr
	if e = cmd.Run(); e != nil {
		t.Fatalf("anonymous pipe peer: %v %s", e, stderr.String())
	}
	var peer ProcessIdentity
	if e = json.Unmarshal(stdout.Bytes(), &peer); e != nil {
		t.Fatal(e)
	}
	parent, e := processIdentity(windows.GetCurrentProcessId())
	if e != nil || peer != parent {
		t.Fatal(peer, parent, e)
	}
	if _, e = stoppedInputParent(bytes.NewReader(nil)); e == nil {
		t.Fatal("non OS pipe accepted")
	}
	request := StoppedPublicationRequest{Parent: parent, Invocation: "00000000000000000000000000000001"}
	body, _ := json.Marshal(request)
	foreign := parent
	foreign.Created++
	response := runStoppedPublicationFromParent(bytes.NewReader(body), foreign)
	if response.Error == "" || response.Outcome.State != duplex.NoWrite {
		t.Fatal(response)
	}
	// Exercise the real production dispatcher with a valid parent pipe and
	// request binding. It must keep the existing unqualified profile closed.
	request.ExecutableSHA256 = "unreviewed"
	request.Build = retailReloadSourceBuild
	request.Product = "retail"
	body, _ = json.Marshal(request)
	cmd = exec.CommandContext(ctx, exe, stoppedPublisherArg)
	stdout.Reset()
	stderr.Reset()
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr
	cmd.Stdin = bytes.NewReader(body)
	if e = cmd.Run(); e != nil {
		t.Fatalf("production dispatcher: %v %s", e, stderr.String())
	}
	if e = json.Unmarshal(stdout.Bytes(), &response); e != nil {
		t.Fatal(e)
	}
	if response.Error != "live.duplex_writer_profile_unverified" || response.Invocation != request.Invocation || response.Outcome.State != duplex.NoWrite || response.Stop.DebuggerThread != 0 {
		t.Fatal(response)
	}
}
