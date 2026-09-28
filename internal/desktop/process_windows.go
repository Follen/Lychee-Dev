//go:build windows

package desktop

import (
	"context"
	"errors"
	"golang.org/x/sys/windows"
)

// ProcessEnded proves the selected process lifetime ended. Access failures and
// missing windows are not evidence of process exit. PID reuse is not continuity.
func ProcessEnded(ctx context.Context, expected WindowIdentity) (string, error) {
	if err := ctx.Err(); err != nil {
		return "", err
	}
	if expected.ProcessID == 0 || expected.ProcessStartedAt == 0 {
		return "", ErrIdentityChanged
	}
	h, err := windows.OpenProcess(windows.PROCESS_QUERY_LIMITED_INFORMATION|windows.SYNCHRONIZE, false, expected.ProcessID)
	if errors.Is(err, windows.ERROR_INVALID_PARAMETER) {
		return "process_absent", nil
	}
	if err != nil {
		return "", err
	}
	defer windows.CloseHandle(h)
	var created, exited, kernel, user windows.Filetime
	if err = windows.GetProcessTimes(h, &created, &exited, &kernel, &user); err != nil {
		return "", err
	}
	started := uint64(created.HighDateTime)<<32 | uint64(created.LowDateTime)
	if started != expected.ProcessStartedAt {
		return "pid_reused", nil
	}
	state, err := windows.WaitForSingleObject(h, 0)
	if err != nil {
		return "", err
	}
	if state == windows.WAIT_OBJECT_0 {
		return "process_exited", nil
	}
	if state != uint32(windows.WAIT_TIMEOUT) {
		return "", ErrIdentityChanged
	}
	return "", nil
}
