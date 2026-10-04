//go:build windows && amd64

package memory

import (
	"bytes"
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"os/exec"
	"strings"
	"syscall"
	"time"
	"unsafe"

	"github.com/follenfang/lycheedev/internal/live/duplex"
	"golang.org/x/sys/windows"
)

const stoppedPublisherArg = "--lycheedev-mailbox-stopped-publisher-v1"
const stoppedSentinelArg = "--lycheedev-mailbox-stopped-sentinel-v1"
const stoppedRequestLimit = 2 << 20
const stoppedResponseLimit = 64 << 10
const maxStoppedInterval = 250 * time.Millisecond

// A watchdog terminates only this dedicated helper. In particular it never
// detaches the target behind a thread that might still be inside WPM.
func stoppedWatchdog(ctx context.Context) func() {
	done := make(chan struct{})
	go func() {
		select {
		case <-done:
			return
		case <-ctx.Done():
			_ = windows.TerminateProcess(windows.CurrentProcess(), 6)
		}
	}()
	return func() { close(done) }
}

// A failed detach leaves the timer armed across all remaining cleanup/output.
// Cancellation is also withheld: it must not race with disarming the only
// protection against a blocked cleanup while the target is still attached.
func finishStoppedWatchdog(returned bool, stop StoppedObservation, disarm func(), cancel context.CancelFunc) {
	if returned && (!stop.Attached || stop.Detached) {
		disarm()
		cancel()
	}
}

// StoppedPublicationRequest contains only protocol identities and a message;
// the helper resolves all addresses itself. Deployment/claim leases and durable
// intent remain the host's responsibility before invoking this operation.
type StoppedPublicationRequest struct {
	Parent           ProcessIdentity `json:"parent"`
	Invocation       string          `json:"invocation"`
	Target           ProcessIdentity `json:"target"`
	ExecutableSHA256 string          `json:"executableSHA256"`
	Build            string          `json:"build"`
	Product          string          `json:"product"`
	Release          string          `json:"release"`
	ActorGUID        string          `json:"actorGUID"`
	Message          duplex.Message  `json:"message"`
	DeadlineUnixNano int64           `json:"deadlineUnixNano"`
}

type stoppedPublicationResponse struct {
	Invocation string              `json:"invocation"`
	Outcome    duplex.WriteOutcome `json:"outcome"`
	Ranges     []DuplexWriteRange  `json:"ranges"`
	Stop       StoppedObservation  `json:"stop"`
	Error      string              `json:"error,omitempty"`
}

type cappedStopOutput struct {
	bytes.Buffer
	overflow bool
}

func (b *cappedStopOutput) Write(p []byte) (int, error) {
	if b.Len()+len(p) > stoppedResponseLimit {
		b.overflow = true
		return 0, errors.New("memory.helper_output_limit")
	}
	return b.Buffer.Write(p)
}

// PublishStoppedDuplexRow runs WPM in the same short-lived process that owns
// the debug connection. KillOnExit(FALSE) makes helper death detach its target.
// CommandContext terminates and Wait drains the helper on cancellation; no
// uncertain write is retried. This does not make WPM atomic or qualify an ABI.
func PublishStoppedDuplexRow(ctx context.Context, request StoppedPublicationRequest) (duplex.WriteOutcome, []DuplexWriteRange, StoppedObservation, error) {
	noWrite := duplex.WriteOutcome{State: duplex.NoWrite}
	deadline, ok := ctx.Deadline()
	if !ok {
		return noWrite, nil, StoppedObservation{}, errors.New("memory.write_deadline_required")
	}
	if e := ctx.Err(); e != nil {
		return noWrite, nil, StoppedObservation{}, e
	}
	profile := DuplexWriteCapability(request.ExecutableSHA256, request.Build, request.Product)
	if !profile.Eligible || profile.Mode != "stopped" {
		return noWrite, nil, StoppedObservation{}, errors.New("live.duplex_writer_profile_unverified")
	}
	if _, e := duplex.EncodeMessage(request.Message); e != nil {
		return noWrite, nil, StoppedObservation{}, e
	}
	request.DeadlineUnixNano = deadline.UnixNano()
	parent, e := processIdentity(windows.GetCurrentProcessId())
	if e != nil {
		return noWrite, nil, StoppedObservation{}, e
	}
	request.Parent = parent
	var invocation [16]byte
	if _, e = rand.Read(invocation[:]); e != nil {
		return noWrite, nil, StoppedObservation{}, e
	}
	request.Invocation = hex.EncodeToString(invocation[:])
	body, e := json.Marshal(request)
	if e != nil || len(body) > stoppedRequestLimit {
		return noWrite, nil, StoppedObservation{}, errors.Join(errors.New("memory.helper_request_invalid"), e)
	}
	exe, e := os.Executable()
	if e != nil {
		return noWrite, nil, StoppedObservation{}, e
	}
	cmd := exec.CommandContext(ctx, exe, stoppedPublisherArg)
	cmd.SysProcAttr = &syscall.SysProcAttr{CreationFlags: windows.CREATE_NO_WINDOW}
	cmd.Stdin = bytes.NewReader(body)
	var output, diagnostic cappedStopOutput
	cmd.Stdout = &output
	cmd.Stderr = &diagnostic
	if e = cmd.Start(); e != nil {
		return noWrite, nil, StoppedObservation{}, e
	}
	// After Start, a missing or invalid final response cannot prove no WPM.
	e = cmd.Wait()
	unknown := duplex.WriteOutcome{State: duplex.UnknownWrite}
	if e != nil || output.overflow || diagnostic.overflow {
		return unknown, nil, StoppedObservation{}, errors.Join(ErrStoppedUnavailable, e, ctx.Err())
	}
	var response stoppedPublicationResponse
	decoder := json.NewDecoder(&output.Buffer)
	decoder.DisallowUnknownFields()
	if e = decoder.Decode(&response); e != nil {
		return unknown, nil, StoppedObservation{}, errors.Join(ErrStoppedUnavailable, e)
	}
	if decoder.Decode(new(any)) != io.EOF {
		return unknown, nil, StoppedObservation{}, ErrStoppedUnavailable
	}
	if response.Invocation != request.Invocation {
		return unknown, nil, response.Stop, ErrStoppedUnavailable
	}
	if response.Error != "" {
		return response.Outcome, response.Ranges, response.Stop, fmt.Errorf("%w: %s", ErrStoppedUnavailable, response.Error)
	}
	if !response.Stop.Detached || response.Outcome.State != duplex.CompleteWrite || !response.Outcome.ReadbackVerified {
		return unknown, response.Ranges, response.Stop, ErrStoppedUnavailable
	}
	return response.Outcome, response.Ranges, response.Stop, nil
}

// RunMailboxMemoryHelper must be called at executable entry before normal CLI
// parsing. It recognizes exactly two private roles, never arbitrary debug or
// memory commands. The caller must os.Exit(code) when handled is true.
func RunMailboxMemoryHelper(args []string, input io.Reader, output io.Writer) (handled bool, code int) {
	if len(args) != 1 {
		return false, 0
	}
	switch args[0] {
	case stoppedSentinelArg:
		_, _ = io.WriteString(output, "ready\n")
		_, _ = io.Copy(io.Discard, input)
		return true, 0
	case stoppedPublisherArg:
		parent, e := stoppedInputParent(input)
		if e != nil {
			_ = json.NewEncoder(output).Encode(stoppedPublicationResponse{Outcome: duplex.WriteOutcome{State: duplex.NoWrite}, Error: e.Error()})
			return true, 0
		}
		response := runStoppedPublicationFromParent(input, parent)
		if e := json.NewEncoder(output).Encode(response); e != nil {
			return true, 5
		}
		return true, 0
	default:
		return false, 0
	}
}

// The stdin read endpoint is an inherited anonymous pipe created by this
// executable's actual parent. Anonymous pipes are implemented as named pipes;
// failure to obtain their peer identity is fail-closed. Hidden argv is not an
// authority, and a foreign shell cannot invoke this role with arbitrary JSON.
// This binds invocation/recovery to the normal host driver, not a security
// boundary against a malicious user who can modify/debug the same executable.
func stoppedInputParent(input io.Reader) (ProcessIdentity, error) {
	file, ok := input.(*os.File)
	if !ok {
		return ProcessIdentity{}, ErrStoppedUnavailable
	}
	var peer uint32
	proc := debugKernel.NewProc("GetNamedPipeClientProcessId")
	if e := debugCall(proc, file.Fd(), uintptr(unsafe.Pointer(&peer))); e != nil {
		return ProcessIdentity{}, e
	}
	if peer == 0 || peer == windows.GetCurrentProcessId() {
		return ProcessIdentity{}, ErrStoppedUnavailable
	}
	snapshot, e := windows.CreateToolhelp32Snapshot(windows.TH32CS_SNAPPROCESS, 0)
	if e != nil {
		return ProcessIdentity{}, e
	}
	defer windows.CloseHandle(snapshot)
	entry := windows.ProcessEntry32{Size: uint32(unsafe.Sizeof(windows.ProcessEntry32{}))}
	if e = windows.Process32First(snapshot, &entry); e != nil {
		return ProcessIdentity{}, e
	}
	found := false
	for count := 0; count < 65536; count++ {
		if entry.ProcessID == windows.GetCurrentProcessId() {
			found = entry.ParentProcessID == peer
			break
		}
		if e = windows.Process32Next(snapshot, &entry); e != nil {
			break
		}
	}
	if !found {
		return ProcessIdentity{}, ErrStoppedUnavailable
	}
	parent, e := processIdentity(peer)
	if e != nil {
		return ProcessIdentity{}, e
	}
	self, e := processIdentity(windows.GetCurrentProcessId())
	if e != nil {
		return ProcessIdentity{}, e
	}
	if !strings.EqualFold(parent.Image, self.Image) {
		return ProcessIdentity{}, ErrStoppedUnavailable
	}
	return parent, nil
}

func processIdentity(pid uint32) (ProcessIdentity, error) {
	h, e := windows.OpenProcess(windows.PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
	if e != nil {
		return ProcessIdentity{}, e
	}
	defer windows.CloseHandle(h)
	var c, x, k, u windows.Filetime
	if e = windows.GetProcessTimes(h, &c, &x, &k, &u); e != nil {
		return ProcessIdentity{}, e
	}
	buf := make([]uint16, 32768)
	n := uint32(len(buf))
	if e = windows.QueryFullProcessImageName(h, 0, &buf[0], &n); e != nil {
		return ProcessIdentity{}, e
	}
	return ProcessIdentity{PID: pid, Created: uint64(c.HighDateTime)<<32 | uint64(c.LowDateTime), Image: windows.UTF16ToString(buf[:n])}, nil
}

// The sentinel is our own executable with no target data. If we die before the
// FALSE policy call, only this owned sentinel is subject to the default policy.
func startStoppedSentinel(ctx context.Context) (ProcessIdentity, func(), error) {
	exe, e := os.Executable()
	if e != nil {
		return ProcessIdentity{}, nil, e
	}
	cmd := exec.CommandContext(ctx, exe, stoppedSentinelArg)
	cmd.SysProcAttr = &syscall.SysProcAttr{CreationFlags: windows.CREATE_NO_WINDOW}
	stdin, e := cmd.StdinPipe()
	if e != nil {
		return ProcessIdentity{}, nil, e
	}
	stdout, e := cmd.StdoutPipe()
	if e != nil {
		stdin.Close()
		return ProcessIdentity{}, nil, e
	}
	if e = cmd.Start(); e != nil {
		stdin.Close()
		return ProcessIdentity{}, nil, e
	}
	cleanup := func() { stdin.Close(); _ = cmd.Process.Kill(); _ = cmd.Wait() }
	var ready [6]byte
	if _, e = io.ReadFull(stdout, ready[:]); e != nil || string(ready[:]) != "ready\n" {
		cleanup()
		return ProcessIdentity{}, nil, errors.Join(ErrStoppedUnavailable, e)
	}
	id, e := processIdentity(uint32(cmd.Process.Pid))
	if e != nil {
		cleanup()
		return ProcessIdentity{}, nil, e
	}
	return id, cleanup, nil
}

func runStoppedPublication(input io.Reader) (response stoppedPublicationResponse) {
	return runStoppedPublicationFromParent(input, ProcessIdentity{})
}

func runStoppedPublicationFromParent(input io.Reader, parent ProcessIdentity) (response stoppedPublicationResponse) {
	response.Outcome.State = duplex.NoWrite
	fail := func(e error) stoppedPublicationResponse { response.Error = e.Error(); return response }
	body, e := io.ReadAll(io.LimitReader(input, stoppedRequestLimit+1))
	if e != nil || len(body) > stoppedRequestLimit {
		return fail(errors.Join(ErrStoppedUnavailable, e))
	}
	var request StoppedPublicationRequest
	d := json.NewDecoder(bytes.NewReader(body))
	d.DisallowUnknownFields()
	if e = d.Decode(&request); e != nil {
		return fail(e)
	}
	if d.Decode(new(any)) != io.EOF {
		return fail(ErrStoppedUnavailable)
	}
	response.Invocation = request.Invocation
	if parent.PID == 0 || parent != request.Parent || len(request.Invocation) != 32 {
		return fail(ErrStoppedUnavailable)
	}
	if decoded, e := hex.DecodeString(request.Invocation); e != nil || len(decoded) != 16 || hex.EncodeToString(decoded) != request.Invocation {
		return fail(ErrStoppedUnavailable)
	}
	profile := DuplexWriteCapability(request.ExecutableSHA256, request.Build, request.Product)
	if !profile.Eligible || profile.Mode != "stopped" {
		return fail(errors.New("live.duplex_writer_profile_unverified"))
	}
	if _, e = duplex.EncodeMessage(request.Message); e != nil {
		return fail(e)
	}
	deadline := time.Unix(0, request.DeadlineUnixNano)
	if !deadline.After(time.Now()) || request.Release == "" {
		return fail(ErrStoppedUnavailable)
	}
	ctx, cancel := context.WithDeadline(context.Background(), deadline)
	defer cancel()
	p, e := OpenDuplexWriter(request.Target.PID, request.Target.Created, request.Target.Image)
	if e != nil {
		return fail(e)
	}
	defer p.Close()
	module, e := p.MainModule(ctx)
	if e != nil {
		return fail(e)
	}
	if module.ExecutableSHA256 != request.ExecutableSHA256 {
		return fail(errors.New("memory.image_changed"))
	}
	readModule := func(c context.Context, at uint64, b []byte) (int, error) { return p.ReadModule(c, module, at, b) }
	reload, e := ResolveReloadState(ctx, module.Base, module.Size, module.ExecutableSHA256, request.Build, request.Product, module.LuaImageLayout(), readModule)
	if e != nil {
		return fail(e)
	}
	reader, e := OpenLuaMailbox(ctx, p, module.Base, module.Size, module.ExecutableSHA256, request.Release, module.LuaImageLayout(), readModule)
	if e != nil {
		return fail(e)
	}
	sentinel, cleanup, e := startStoppedSentinel(ctx)
	if e != nil {
		return fail(e)
	}
	defer cleanup()
	// A blocked callback or API cannot strand the target: this watchdog ends
	// the helper, and the OS detaches under the already configured FALSE policy.
	// The timer starts before any debug attachment. It never resumes another
	// process while this helper might still have an in-flight WPM.
	stopCtx, stopCancel := context.WithTimeout(ctx, maxStoppedInterval)
	disarm := stoppedWatchdog(stopCtx)
	stopReturned := false
	defer func() { finishStoppedWatchdog(stopReturned, response.Stop, disarm, stopCancel) }()
	response.Stop, e = withStoppedProcess(stopCtx, request.Target, sentinel, func(grant *stoppedProcess) error {
		guard := func(c context.Context) error {
			if e := grant.verify(c); e != nil {
				return e
			}
			o, e := reload.Observe(c)
			if e != nil {
				return e
			}
			if request.Message.Header.Kind == duplex.Frame {
				e = o.CheckBusinessWriteGate()
			} else {
				e = o.CheckWriteGate()
			}
			if e != nil {
				return e
			}
			b, e := reader.ReadDuplexString(c, []DuplexPath{{Name: "sendbox"}, {Name: "status"}}, duplex.MaxSendboxBytes+44)
			if e != nil {
				return e
			}
			s, e := duplex.DecodeSendbox(b)
			if e != nil {
				return e
			}
			if s.Build != request.Build || s.Product != request.Product || s.Release != request.Release {
				return duplex.ErrIdentity
			}
			h := request.Message.Header
			if s.Runtime != h.Runtime || s.Arena != h.Arena || s.ActorBinding != h.ActorBinding {
				return duplex.ErrIdentity
			}
			if h.Kind == duplex.Frame {
				if request.ActorGUID == "" || s.ActorGUID != request.ActorGUID {
					return duplex.ErrIdentity
				}
				return duplex.ValidateFrameTransition(s, request.Message)
			}
			matched := s.Session == h.Session && s.Owner == h.Owner && s.Fence == h.Fence
			if !matched {
				candidate, e := duplex.NextIdentity(s, h.Owner, h.Session)
				matched = e == nil && candidate == (duplex.Identity{Runtime: h.Runtime, Arena: h.Arena, Session: h.Session, Owner: h.Owner, ActorBinding: h.ActorBinding, Fence: h.Fence}) && (s.ReadyChallenge == h.Challenge || h.Kind == duplex.Repair && s.Repair != nil && s.Repair.Challenge == h.Challenge)
			}
			if !matched {
				return duplex.ErrIdentity
			}
			if !s.ControlReady {
				return duplex.ErrPending
			}
			return nil
		}
		var e error
		response.Outcome, response.Ranges, e = p.PublishDuplexRow(stopCtx, reader, request.Message, guard)
		return e
	})
	stopReturned = true
	if response.Stop.Attached && !response.Stop.Detached {
		response.Outcome = duplex.WriteOutcome{State: duplex.UnknownWrite, Bytes: response.Outcome.Bytes}
	}
	if e != nil {
		return fail(e)
	}
	return response
}
