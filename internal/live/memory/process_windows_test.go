//go:build windows && amd64

package memory

import (
	"bytes"
	"context"
	"encoding/json"
	"io"
	"os"
	"os/exec"
	"syscall"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
	"golang.org/x/sys/windows"
)

type childMemory struct {
	PID     uint32
	Created uint64
	Image   string
	Address uint64
	Header  bridge.MemoryHeader
}

// Only this test child allocates/writes its own memory. The parent exercises
// the same read-only process adapter used by live, without a game or wowdump.
func TestNativeMemoryChild(t *testing.T) {
	if os.Getenv("LYCHEE_MEMORY_CHILD") != "1" {
		t.Skip("subprocess entry")
	}
	address, err := windows.VirtualAlloc(0, 2<<20, windows.MEM_COMMIT|windows.MEM_RESERVE, windows.PAGE_READWRITE)
	if err != nil {
		t.Fatal(err)
	}
	defer windows.VirtualFree(address, 0, windows.MEM_RELEASE)
	h := bridge.MemoryHeader{Kind: bridge.MemoryReceipt, State: 1, Sequence: 7}
	for i := range h.Nonce {
		h.Nonce[i] = byte(31 + i)
		h.Runtime[i] = byte(91 + i)
		h.Ticket[i] = byte(141 + i)
	}
	record, err := bridge.EncodeMemoryRecord(h, []byte(`{"native":"荔枝<&>","ok":true}`))
	if err != nil {
		t.Fatal(err)
	}
	at := (1 << 20) - 7
	var written uintptr
	if err = windows.WriteProcessMemory(windows.CurrentProcess(), address+uintptr(at), &record[0], uintptr(len(record)), &written); err != nil || written != uintptr(len(record)) {
		t.Fatalf("child allocation: %d %v", written, err)
	}
	var c, x, k, u windows.Filetime
	if err = windows.GetProcessTimes(windows.CurrentProcess(), &c, &x, &k, &u); err != nil {
		t.Fatal(err)
	}
	image, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	info := childMemory{uint32(os.Getpid()), uint64(c.HighDateTime)<<32 | uint64(c.LowDateTime), image, uint64(address) + uint64(at), h}
	if err = json.NewEncoder(os.Stdout).Encode(info); err != nil {
		t.Fatal(err)
	}
	_, _ = io.Copy(io.Discard, os.Stdin)
}

func TestNativeChildScanCacheOffAndStaleProcessIdentity(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()
	command := exec.CommandContext(ctx, os.Args[0], "-test.run=^TestNativeMemoryChild$")
	command.Env = append(os.Environ(), "LYCHEE_MEMORY_CHILD=1")
	command.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}
	input, err := command.StdinPipe()
	if err != nil {
		t.Fatal(err)
	}
	output, err := command.StdoutPipe()
	if err != nil {
		t.Fatal(err)
	}
	var diagnostics bytes.Buffer
	command.Stderr = &diagnostics
	if err = command.Start(); err != nil {
		t.Fatal(err)
	}
	defer func() {
		input.Close()
		if err := command.Wait(); err != nil {
			t.Errorf("child: %v %s", err, diagnostics.String())
		}
	}()
	var child childMemory
	if err = json.NewDecoder(output).Decode(&child); err != nil {
		t.Fatal(err)
	}
	if wrong, err := Open(child.PID, child.Created+1, child.Image); err == nil {
		wrong.Close()
		t.Fatal("stale PID generation accepted")
	}
	process, err := Open(child.PID, child.Created, child.Image)
	if err != nil {
		t.Fatal(err)
	}
	defer process.Close()
	selector := Selector{Nonce: child.Header.Nonce, Runtime: child.Header.Runtime, Ticket: child.Header.Ticket, Kind: bridge.MemoryReceipt}
	point, err := ReadRecord(ctx, process, child.Address, selector)
	if err != nil {
		t.Fatal(err)
	}
	found, err := Find(ctx, process, selector, nil, false)
	if err != nil {
		t.Fatal(err)
	}
	if !found.Coverage.Complete || len(found.Coverage.Workers) != 8 {
		t.Fatalf("coverage=%+v", found.Coverage)
	}
	matched := false
	for _, record := range found.Records {
		if record.Address == child.Address && bytes.Equal(record.Payload, point.Payload) {
			matched = true
		}
	}
	if !matched {
		t.Fatal("native chunk-boundary allocation was missed")
	}
}
