//go:build windows

package luals

import (
	"bufio"
	"context"
	"errors"
	"os/exec"
	"strconv"
	"strings"
	"testing"
	"time"

	"golang.org/x/sys/windows"
)

// An in-flight job cancellation must terminate both the owner and a child.
func TestWindowsJobClosesProcessTree(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	script := `$child = Start-Process -WindowStyle Hidden -FilePath powershell.exe -ArgumentList '-NoProfile -Command Start-Sleep 30' -PassThru; Write-Output $child.Id; Start-Sleep 30`
	command := exec.CommandContext(ctx, "powershell.exe", "-NoProfile", "-Command", script)
	hideProcess(command)
	stdout, err := command.StdoutPipe()
	if err != nil {
		t.Fatal(err)
	}
	closeJob, err := startGuarded(command)
	if err != nil {
		t.Fatal(err)
	}
	defer closeJob()
	line, err := bufio.NewReader(stdout).ReadString('\n')
	if err != nil {
		cancel()
		_ = command.Wait()
		t.Fatal(err)
	}
	pid64, err := strconv.ParseUint(strings.TrimSpace(line), 10, 32)
	if err != nil {
		cancel()
		_ = command.Wait()
		t.Fatal(err)
	}
	child, err := windows.OpenProcess(windows.PROCESS_QUERY_LIMITED_INFORMATION, false, uint32(pid64))
	if err != nil {
		cancel()
		_ = command.Wait()
		t.Fatal(err)
	}
	defer windows.CloseHandle(child)
	cancel()
	closeJob()
	done := make(chan error, 1)
	go func() { done <- command.Wait() }()
	select {
	case <-done:
	case <-time.After(5 * time.Second):
		t.Fatal("owner did not exit after cancellation")
	}
	deadline := time.Now().Add(5 * time.Second)
	for {
		var status uint32
		if err := windows.GetExitCodeProcess(child, &status); err != nil {
			t.Fatal(err)
		}
		if status != 259 {
			break
		} // STILL_ACTIVE
		if time.Now().After(deadline) {
			t.Fatal("worker child remained alive after job close")
		}
		time.Sleep(20 * time.Millisecond)
	}
	if !errors.Is(ctx.Err(), context.Canceled) {
		t.Fatalf("context: %v", ctx.Err())
	}
}
