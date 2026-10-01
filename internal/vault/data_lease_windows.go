//go:build windows

package vault

import (
	"context"
	"errors"
	"fmt"
	"os"

	"golang.org/x/sys/windows"
)

// Data admission uses an overlapped kernel lock, independently of the shared
// lease wait policy used by live. Closing the returned handle releases its lock
// on normal exit and process death; the lock file is never removed.
func acquireDataLease(ctx context.Context, scope, resource string) (*Lease, error) {
	seed, err := openLease(ctx, scope, resource)
	if err != nil {
		return nil, err
	}
	path := seed.Name()
	if err = seed.Close(); err != nil {
		return nil, err
	}
	name, err := windows.UTF16PtrFromString(path)
	if err != nil {
		return nil, err
	}
	handle, err := windows.CreateFile(name, windows.GENERIC_READ|windows.GENERIC_WRITE, windows.FILE_SHARE_READ|windows.FILE_SHARE_WRITE, nil, windows.OPEN_EXISTING, windows.FILE_ATTRIBUTE_NORMAL|windows.FILE_FLAG_OVERLAPPED, 0)
	if err != nil {
		return nil, err
	}
	file := os.NewFile(uintptr(handle), path)
	keep := false
	defer func() {
		if !keep {
			file.Close()
		}
	}()
	event, err := windows.CreateEvent(nil, 1, 0, nil)
	if err != nil {
		return nil, err
	}
	defer windows.CloseHandle(event)
	overlapped := &windows.Overlapped{HEvent: event}
	if err = ctx.Err(); err != nil {
		return nil, err
	}
	err = windows.LockFileEx(handle, windows.LOCKFILE_EXCLUSIVE_LOCK, 0, 1, 0, overlapped)
	if errors.Is(err, windows.ERROR_IO_PENDING) {
		cancelDone := make(chan struct{})
		stop := context.AfterFunc(ctx, func() {
			_ = windows.CancelIoEx(handle, overlapped)
			close(cancelDone)
		})
		// Join the callback before either handle or OVERLAPPED can be reclaimed.
		defer func() {
			if !stop() {
				<-cancelDone
			}
		}()
		state, waitErr := windows.WaitForSingleObject(event, windows.INFINITE)
		if waitErr != nil || state != windows.WAIT_OBJECT_0 {
			_ = windows.CancelIoEx(handle, overlapped)
		}
		// CancelIoEx only requests cancellation. Complete the operation before
		// returning, including an acquisition that won the cancellation race.
		var transferred uint32
		err = windows.GetOverlappedResult(handle, overlapped, &transferred, true)
		if waitErr != nil {
			err = errors.Join(waitErr, err)
		} else if state != windows.WAIT_OBJECT_0 {
			err = errors.Join(fmt.Errorf("vault: data lease wait state %d", state), err)
		}
	}
	if canceled := ctx.Err(); canceled != nil {
		return nil, canceled
	}
	if err != nil {
		return nil, err
	}
	keep = true
	return &Lease{file: file}, nil
}
