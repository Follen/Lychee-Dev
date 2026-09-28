//go:build windows

package desktop

import (
	"context"
	"golang.org/x/sys/windows"
	"os"
	"os/exec"
	"testing"
)

func processTestIdentity(t *testing.T, pid uint32) WindowIdentity {
	t.Helper()
	h, e := windows.OpenProcess(windows.PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
	if e != nil {
		t.Fatal(e)
	}
	defer windows.CloseHandle(h)
	var c, x, k, u windows.Filetime
	if e = windows.GetProcessTimes(h, &c, &x, &k, &u); e != nil {
		t.Fatal(e)
	}
	return WindowIdentity{ProcessID: pid, ProcessStartedAt: uint64(c.HighDateTime)<<32 | uint64(c.LowDateTime)}
}
func TestProcessEndedUsesLifetimeNotWindow(t *testing.T) {
	ctx := context.Background()
	id := processTestIdentity(t, uint32(os.Getpid()))
	if reason, e := ProcessEnded(ctx, id); e != nil || reason != "" {
		t.Fatalf("live: %s %v", reason, e)
	}
	id.ProcessStartedAt--
	if reason, e := ProcessEnded(ctx, id); e != nil || reason != "pid_reused" {
		t.Fatalf("reused: %s %v", reason, e)
	}
	if _, e := ProcessEnded(ctx, WindowIdentity{}); e == nil {
		t.Fatal("invalid identity treated as dead")
	}
	child := exec.Command("ping.exe", "-n", "30", "127.0.0.1")
	if e := child.Start(); e != nil {
		t.Fatal(e)
	}
	defer child.Process.Kill()
	ended := processTestIdentity(t, uint32(child.Process.Pid))
	if e := child.Process.Kill(); e != nil {
		t.Fatal(e)
	}
	_ = child.Wait()
	if reason, e := ProcessEnded(ctx, ended); e != nil || reason == "" {
		t.Fatalf("exited: %s %v", reason, e)
	}
	cancelled, cancel := context.WithCancel(ctx)
	cancel()
	if _, e := ProcessEnded(cancelled, ended); e == nil {
		t.Fatal("cancel ignored")
	}
}
