//go:build windows

package desktop

import (
	"context"
	"errors"
	"fmt"
	"runtime"

	"golang.org/x/sys/windows"
)

func inputMutexName(target WindowIdentity) string {
	// Local names span cooperating processes in the same interactive Windows
	// session, independently of checkout/workspace paths. HWND alone is reusable.
	return fmt.Sprintf(`Local\LycheeDev.Input.%d.%d.%d`, target.ProcessID, target.ProcessStartedAt, target.Handle)
}

func withInputLock(ctx context.Context, target WindowIdentity, send func() (InputReceipt, error)) (receipt InputReceipt, err error) {
	if err = ConfirmWindow(ctx, target); err != nil {
		return receipt, err
	}
	name, err := windows.UTF16PtrFromString(inputMutexName(target))
	if err != nil {
		return receipt, err
	}
	mutex, err := windows.CreateMutex(nil, false, name)
	if err != nil && !errors.Is(err, windows.ERROR_ALREADY_EXISTS) {
		return receipt, err
	}
	defer func() { err = errors.Join(err, windows.CloseHandle(mutex)) }()
	// Win32 mutex ownership is OS-thread-local, not goroutine-local.
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()
	status, err := windows.WaitForSingleObject(mutex, 0)
	if err != nil {
		return receipt, err
	}
	switch status {
	case windows.WAIT_OBJECT_0:
		defer func() { err = errors.Join(err, windows.ReleaseMutex(mutex)) }()
	case windows.WAIT_ABANDONED:
		return receipt, errors.Join(errors.New("desktop.input_abandoned"), windows.ReleaseMutex(mutex))
	case uint32(windows.WAIT_TIMEOUT):
		// Do not queue behind another command with a now-stale readiness claim.
		return receipt, errors.New("desktop.input_busy")
	default:
		return receipt, fmt.Errorf("desktop.input_lock_status: %d", status)
	}
	if err = ConfirmWindow(ctx, target); err != nil {
		return receipt, err
	}
	return send()
}
