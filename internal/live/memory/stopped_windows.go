//go:build windows && amd64

package memory

import (
	"context"
	"encoding/binary"
	"errors"
	"fmt"
	"runtime"
	"time"
	"unsafe"

	"golang.org/x/sys/windows"
)

// ProcessIdentity fixes the target before establishing a debug relationship.
// A PID alone is never an identity or permission to write.
type ProcessIdentity struct {
	PID     uint32 `json:"pid"`
	Created uint64 `json:"created,string"`
	Image   string `json:"image"`
}

type StoppedObservation struct {
	Attached       bool   `json:"attached"`
	DebuggerThread uint32 `json:"debuggerThread"`
	AttachMicros   int64  `json:"attachMicros"`
	StoppedMicros  int64  `json:"stoppedMicros"`
	Detached       bool   `json:"detached"`
}

var ErrStoppedUnavailable = errors.New("live.duplex_stopped_writer_unavailable")

var debugKernel = windows.NewLazySystemDLL("kernel32.dll")
var debugAttach = debugKernel.NewProc("DebugActiveProcess")
var debugDetach = debugKernel.NewProc("DebugActiveProcessStop")
var debugKillOnExit = debugKernel.NewProc("DebugSetProcessKillOnExit")
var debugWait = debugKernel.NewProc("WaitForDebugEvent")
var debugContinue = debugKernel.NewProc("ContinueDebugEvent")

// Win64 DEBUG_EVENT has a 16-byte prefix and a 160-byte, 8-aligned union.
type stoppedDebugEvent struct {
	Code, PID, TID, pad uint32
	Data                [20]uint64
}

func debugCall(proc *windows.LazyProc, args ...uintptr) error {
	r, _, e := proc.Call(args...)
	if r == 0 {
		return fmt.Errorf("%w: %s: %w", ErrStoppedUnavailable, proc.Name, e)
	}
	return nil
}

func closeDebugFile(e *stoppedDebugEvent) {
	if e.Code == 3 || e.Code == 6 { // CREATE_PROCESS or LOAD_DLL hFile
		if h := windows.Handle(e.Data[0]); h != 0 && h != windows.InvalidHandle {
			_ = windows.CloseHandle(h)
		}
	}
}

// stoppedProcess is never exported or serialized. The grant exists only on
// the debugger OS thread while the first target event remains uncontinued.
type stoppedProcess struct {
	process *Process
	thread  uint32
	active  bool
}

func (s *stoppedProcess) verify(ctx context.Context) error {
	if s == nil || !s.active || windows.GetCurrentThreadId() != s.thread {
		return ErrStoppedUnavailable
	}
	return s.process.Verify(ctx)
}

// withStoppedProcess must run in the dedicated publisher helper. Its caller
// supplies an already started, owned sentinel and a watchdog that terminates
// this helper if the callback cannot return. No SuspendThread counts are used.
// Kill-on-exit FALSE is established on the sentinel before target attachment;
// the sentinel connection stays attached throughout the target grant.
func withStoppedProcess(ctx context.Context, target, sentinel ProcessIdentity, fn func(*stoppedProcess) error) (obs StoppedObservation, returned error) {
	return withStoppedProcessDetach(ctx, target, sentinel, fn, func(pid uint32) error { return debugCall(debugDetach, uintptr(pid)) })
}

// The detach seam is private and used by owned-process fault tests only.
func withStoppedProcessDetach(ctx context.Context, target, sentinel ProcessIdentity, fn func(*stoppedProcess) error, detach func(uint32) error) (obs StoppedObservation, returned error) {
	if fn == nil || target.PID == sentinel.PID || target.PID == windows.GetCurrentProcessId() {
		return obs, ErrStoppedUnavailable
	}
	if _, ok := ctx.Deadline(); !ok {
		return obs, errors.New("memory.write_deadline_required")
	}
	p, err := OpenDuplexWriter(target.PID, target.Created, target.Image)
	if err != nil {
		return obs, err
	}
	defer p.Close()
	owned, err := Open(sentinel.PID, sentinel.Created, sentinel.Image)
	if err != nil {
		return obs, err
	}
	defer owned.Close()
	runtime.LockOSThread()
	// A failed detach cannot safely return this OS thread to the runtime pool.
	// This function is helper-only: its dispatcher always exits the process.
	detachOK := true
	defer func() {
		if detachOK {
			runtime.UnlockOSThread()
		}
	}()
	obs.DebuggerThread = windows.GetCurrentThreadId()
	if err = debugCall(debugAttach, uintptr(sentinel.PID)); err != nil {
		return obs, err
	}
	defer func() {
		e := detach(sentinel.PID)
		if e != nil {
			detachOK = false
		}
		returned = errors.Join(returned, e)
	}()
	if err = debugCall(debugKillOnExit, 0); err != nil {
		return obs, err
	}
	if err = p.Verify(ctx); err != nil {
		return obs, err
	}
	start := time.Now()
	if err = debugCall(debugAttach, uintptr(target.PID)); err != nil {
		return obs, err
	}
	obs.Attached = true
	grant := &stoppedProcess{process: p, thread: obs.DebuggerThread}
	var stoppedAt time.Time
	defer func() {
		grant.active = false
		e := detach(target.PID)
		obs.Detached = e == nil
		if e != nil {
			detachOK = false
		}
		if !stoppedAt.IsZero() {
			obs.StoppedMicros = time.Since(stoppedAt).Microseconds()
		}
		returned = errors.Join(returned, e)
	}()
	initialSentinelBreakpoint := false
	for {
		if err = ctx.Err(); err != nil {
			return obs, err
		}
		var event stoppedDebugEvent
		if err = debugCall(debugWait, uintptr(unsafe.Pointer(&event)), 10); err != nil {
			if errors.Is(err, windows.ERROR_SEM_TIMEOUT) {
				continue
			}
			return obs, err
		}
		closeDebugFile(&event)
		if event.PID == sentinel.PID {
			status := uintptr(0x10002) // DBG_CONTINUE for non-exception events
			if event.Code == 1 {
				// Only the attach-generated first breakpoint belongs to us.
				code := binary.LittleEndian.Uint32((*[160]byte)(unsafe.Pointer(&event.Data[0]))[:4])
				if code == 0x80000003 && !initialSentinelBreakpoint {
					initialSentinelBreakpoint = true
				} else {
					status = 0x80010001
				}
			}
			if err = debugCall(debugContinue, uintptr(event.PID), uintptr(event.TID), status); err != nil {
				return obs, err
			}
			continue
		}
		if event.PID != target.PID || event.Code != 3 {
			return obs, ErrStoppedUnavailable
		}
		stoppedAt = time.Now()
		obs.AttachMicros = stoppedAt.Sub(start).Microseconds()
		// The event's process handle independently proves that the process we
		// attached is the fixed instance, not a PID reused after preflight.
		eventProcess := &Process{handle: windows.Handle(event.Data[1]), PID: target.PID, Created: target.Created, Image: target.Image}
		if err = eventProcess.Verify(ctx); err != nil {
			return obs, err
		}
		grant.active = true
		if err = grant.verify(ctx); err != nil {
			return obs, err
		}
		return obs, fn(grant)
	}
}
