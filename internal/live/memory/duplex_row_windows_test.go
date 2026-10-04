//go:build windows && amd64

package memory

import (
	"bytes"
	"context"
	"encoding/binary"
	"errors"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/live/duplex"
	"golang.org/x/sys/windows"
)

// This fixture owns the entire heap and writes only the current Go test
// executable. Its constructed Table/TValue ABI is not game layout validation.
func ownedCommandMailbox(t *testing.T) (*Process, *MailboxReader, uint64, uint64) {
	t.Helper()
	p, _, _ := ownedDuplexCell(t)
	heap, err := windows.VirtualAlloc(0, 16<<20, windows.MEM_COMMIT|windows.MEM_RESERVE, windows.PAGE_READWRITE)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { windows.VirtualFree(heap, 0, windows.MEM_RELEASE) })
	f, r, cells := duplexFixtureAt(t, 0x140000000, uint64(heap))
	rowAt, rowBytes := f.allocate(duplexCommandWords * 24)
	copy(rowBytes, numberRow(duplexCommandWords).image)
	h := f.bytes(cells, 72)
	binary.LittleEndian.PutUint64(h[32:40], rowAt)
	binary.LittleEndian.PutUint32(h[64:68], duplexCommandWords)
	h[0x45] = 1
	cal := binary.LittleEndian.Uint64(f.bytes(f.nodes["inbox.calibration"], 8))
	inbox := f.table("owned-inbox", []mailboxFixtureEntry{{"calibration", luaValue{cal, 5}}, {"command", luaValue{cells, 5}}})
	binary.LittleEndian.PutUint64(f.bytes(f.nodes["duplex.inbox"], 8), inbox)
	for _, span := range f.spans {
		writeOwnedRange(t, span.address, span.data)
	}
	r.source = p
	return p, r, cells, rowAt
}

func writeOwnedRange(t *testing.T, at uint64, b []byte) {
	t.Helper()
	var n uintptr
	if err := windows.WriteProcessMemory(windows.CurrentProcess(), uintptr(at), &b[0], uintptr(len(b)), &n); err != nil || n != uintptr(len(b)) {
		t.Fatalf("owned fixture setup write: n=%d error=%v", n, err)
	}
}

func TestWholeRowWindowsPublisherOwnProcess(t *testing.T) {
	p, r, _, rowAt := ownedCommandMailbox(t)
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	m := commandRowMessage(t, bytes.Repeat([]byte{0x42}, duplex.MaxSourceBytes))
	start := time.Now()
	out, facts, err := p.PublishDuplexRow(ctx, r, m, func(context.Context) error { return nil })
	elapsed := time.Since(start)
	if err != nil || out.State != duplex.CompleteWrite || !out.ReadbackVerified || out.Bytes != 6293376 || len(facts) != 1 {
		t.Fatalf("out=%+v facts=%+v error=%v", out, facts, err)
	}
	// Publish a short command after a full command and verify the entire tail.
	m = commandRowMessage(t, []byte("return 1"))
	out, _, err = p.PublishDuplexRow(ctx, r, m, func(context.Context) error { return nil })
	if err != nil || out.State != duplex.CompleteWrite {
		t.Fatal(out, err)
	}
	raw := make([]byte, duplexCommandWords*24)
	if n, e := p.Read(ctx, rowAt, raw); e != nil || n != len(raw) {
		t.Fatal(n, e)
	}
	expected, e := planDuplexRow(m, numberRow(duplexCommandWords))
	if e != nil || !bytes.Equal(raw, expected.image) {
		t.Fatal("actual WPM did not preserve metadata/clear tail", e)
	}
	t.Logf("self-owned 1MiB command snapshot/plan/one-WPM/readback: %s; not game throughput or lifetime validation", elapsed)
}

func TestWholeRowWindowsRefusesUnfrozenLeafAndCancelledGate(t *testing.T) {
	p, r, table, rowAt := ownedCommandMailbox(t)
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	m := commandRowMessage(t, []byte("return 1"))
	initial := make([]byte, 24)
	if _, e := p.Read(ctx, rowAt, initial); e != nil {
		t.Fatal(e)
	}
	writeOwnedRange(t, table+0x45, []byte{0})
	if out, _, e := p.PublishDuplexRow(ctx, r, m, func(context.Context) error { return nil }); out.State != duplex.NoWrite || e == nil {
		t.Fatal("unfrozen row accepted", out, e)
	}
	writeOwnedRange(t, table+0x45, []byte{1})
	gateCalls := 0
	out, _, e := p.PublishDuplexRow(ctx, r, m, func(context.Context) error {
		gateCalls++
		if gateCalls == 2 {
			cancel()
		}
		return nil
	})
	if out.State != duplex.NoWrite || !errors.Is(e, context.Canceled) {
		t.Fatal(out, e)
	}
	checkCtx, done := context.WithTimeout(context.Background(), time.Second)
	defer done()
	got := make([]byte, 24)
	if _, e := p.Read(checkCtx, rowAt, got); e != nil || !bytes.Equal(initial, got) {
		t.Fatal("gate crossed cancellation into WPM", e)
	}
}
