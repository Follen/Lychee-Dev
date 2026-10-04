//go:build windows && amd64

package memory

import (
	"bytes"
	"context"
	"errors"
	"os"
	"testing"
	"time"

	"golang.org/x/sys/windows"
)

// A committed, writable page is not ownership of the object inside that page.
// This fixture owns all memory and targets only its own test executable. It
// schedules allocator reuse at the last cancellation check before the real
// WriteProcessMemory call, after identity, TValue and VirtualQuery checks.
type reuseBeforeWriteContext struct {
	context.Context
	checks int
	reuse  func()
}

func (c *reuseBeforeWriteContext) Err() error {
	c.checks++
	if c.checks == 3 {
		c.reuse()
	}
	return c.Context.Err()
}

func ownedDuplexCell(t *testing.T) (*Process, uintptr, func([]byte)) {
	t.Helper()
	address, err := windows.VirtualAlloc(0, 4096, windows.MEM_COMMIT|windows.MEM_RESERVE, windows.PAGE_READWRITE)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { windows.VirtualFree(address, 0, windows.MEM_RELEASE) })
	cellBytes := make([]byte, 24)
	copy(cellBytes, numericPayload(7))
	cellBytes[8] = 3
	writeOwned := func(b []byte) {
		t.Helper()
		var n uintptr
		if e := windows.WriteProcessMemory(windows.CurrentProcess(), address, &b[0], uintptr(len(b)), &n); e != nil || n != uintptr(len(b)) {
			t.Fatalf("self-owned fixture write: bytes=%d error=%v", n, e)
		}
	}
	writeOwned(cellBytes)
	var created, exited, kernel, user windows.Filetime
	if err = windows.GetProcessTimes(windows.CurrentProcess(), &created, &exited, &kernel, &user); err != nil {
		t.Fatal(err)
	}
	image, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	p, err := OpenDuplexWriter(uint32(os.Getpid()), uint64(created.HighDateTime)<<32|uint64(created.LowDateTime), image)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { p.Close() })
	return p, address, writeOwned
}

func TestDuplexWriteRejectsReloadStartedAfterCellChecks(t *testing.T) {
	p, address, writeOwned := ownedDuplexCell(t)
	cellBytes := make([]byte, 24)
	base, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	foreignPayload := numericPayload(42)
	reloadStarted := false
	ctx := &reuseBeforeWriteContext{Context: base, reuse: func() {
		// Object A is released and object B reuses the same allocator subblock.
		// The mapping, protection, PID and ordinary numeric tag stay unchanged.
		writeOwned(foreignPayload)
		reloadStarted = true
	}}
	n, err := p.WriteDuplexCell(ctx, NumericCell{Address: uint64(address), Value: 7}, 99, func(context.Context) error {
		if reloadStarted {
			return ErrReloadActive
		}
		return nil
	})
	if got, e := p.Read(base, uint64(address), cellBytes); e != nil || got != len(cellBytes) {
		t.Fatalf("self-owned fixture read: bytes=%d error=%v", got, e)
	}
	if ctx.checks >= 3 && !bytes.Equal(cellBytes[:8], foreignPayload) {
		t.Fatalf("replacement object B overwritten: got=%x expected=%x", cellBytes[:8], foreignPayload)
	}
	if n != 0 || !errors.Is(err, ErrReloadActive) || ctx.checks != 3 {
		t.Fatalf("reload gate accepted write: bytes=%d error=%v checks=%d", n, err, ctx.checks)
	}
}

func TestDuplexWriteRechecksCancellationAfterGuard(t *testing.T) {
	p, address, _ := ownedDuplexCell(t)
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	n, err := p.WriteDuplexCell(ctx, NumericCell{Address: uint64(address), Value: 7}, 99, func(context.Context) error {
		cancel()
		return nil
	})
	readCtx, readCancel := context.WithTimeout(context.Background(), time.Second)
	defer readCancel()
	got := make([]byte, 8)
	if _, e := p.Read(readCtx, uint64(address), got); e != nil {
		t.Fatal(e)
	}
	if n != 0 || !errors.Is(err, context.Canceled) || !bytes.Equal(got, numericPayload(7)) {
		t.Fatalf("guard crossed cancellation into write: bytes=%d error=%v payload=%x", n, err, got)
	}
}

// An observation can be true and become stale before WPM. This deliberate
// self-process limitation fixture keeps the remaining ABA race visible; no
// production writer profile may be enabled on the strength of a read gate.
func TestDuplexLifecycleObservationDoesNotPinAllocation(t *testing.T) {
	p, address, writeOwned := ownedDuplexCell(t)
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	n, err := p.WriteDuplexCell(ctx, NumericCell{Address: uint64(address), Value: 7}, 99, func(context.Context) error {
		// Simulate reuse immediately after a passing lifecycle observation.
		writeOwned(numericPayload(42))
		return nil
	})
	got := make([]byte, 8)
	if _, e := p.Read(ctx, uint64(address), got); e != nil {
		t.Fatal(e)
	}
	if n != 8 || err != nil || !bytes.Equal(got, numericPayload(99)) {
		t.Fatalf("self-process ABA premise changed: bytes=%d error=%v payload=%x", n, err, got)
	}
	if DuplexWriteCapability(retailReloadExecutableSHA256, retailReloadSourceBuild, "retail").Eligible {
		t.Fatal("read gate was promoted to allocation lifetime authority")
	}
}

func TestDuplexWriteRequiresLifecycleGuard(t *testing.T) {
	p := &Process{}
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	if n, e := p.WriteDuplexCell(ctx, NumericCell{}, 99, nil); n != 0 || !errors.Is(e, ErrReloadGuardRequired) {
		t.Fatalf("unguarded write accepted: %d %v", n, e)
	}
}
