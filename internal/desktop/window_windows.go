//go:build windows

package desktop

import (
	"context"
	"fmt"
	"golang.org/x/sys/windows"
	"strings"
	"sync"
	"unsafe"
)

var (
	userLibrary      = windows.NewLazySystemDLL("user32.dll")
	enumerateWindows = userLibrary.NewProc("EnumWindows")
	windowProcess    = userLibrary.NewProc("GetWindowThreadProcessId")
	windowTitle      = userLibrary.NewProc("GetWindowTextW")
	windowClass      = userLibrary.NewProc("GetClassNameW")
	windowVisible    = userLibrary.NewProc("IsWindowVisible")
	windowExists     = userLibrary.NewProc("IsWindow")
	// Windows callbacks allocated by NewCallback cannot be freed. Reuse one
	// callback for every enumeration instead of leaking a thunk on each call.
	enumerationMutex sync.Mutex
	enumerationSink  func(uintptr)
	enumerationThunk = windows.NewCallback(func(hwnd, _ uintptr) uintptr {
		if enumerationSink != nil {
			enumerationSink(hwnd)
		}
		return 1
	})
)

func ListWindows(ctx context.Context) ([]WindowIdentity, error) {
	enumerationMutex.Lock()
	defer enumerationMutex.Unlock()
	result := make([]WindowIdentity, 0)
	enumerationSink = func(hwnd uintptr) {
		if ctx.Err() != nil {
			return
		}
		visible, _, _ := windowVisible.Call(hwnd)
		if visible == 0 {
			return
		}
		identity, err := inspectWindow(hwnd)
		if err == nil {
			result = append(result, identity)
		}
	}
	defer func() { enumerationSink = nil }()
	ok, _, err := enumerateWindows.Call(enumerationThunk, 0)
	if ctx.Err() != nil {
		return nil, ctx.Err()
	}
	if ok == 0 {
		return nil, fmt.Errorf("desktop: enumerate windows: %w", err)
	}
	return result, nil
}

func ConfirmWindow(ctx context.Context, expected WindowIdentity) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if expected.Handle == 0 || expected.ProcessID == 0 || expected.ProcessStartedAt == 0 || expected.Executable == "" {
		return ErrIdentityChanged
	}
	actual, err := inspectWindow(uintptr(expected.Handle))
	if err != nil {
		return fmt.Errorf("%w: %v", ErrIdentityChanged, err)
	}
	if actual.ProcessID != expected.ProcessID || actual.ProcessStartedAt != expected.ProcessStartedAt || !strings.EqualFold(actual.Executable, expected.Executable) || actual.Class != expected.Class {
		return ErrIdentityChanged
	}
	return nil
}

func inspectWindow(hwnd uintptr) (WindowIdentity, error) {
	var out WindowIdentity
	exists, _, _ := windowExists.Call(hwnd)
	if exists == 0 {
		return out, ErrIdentityChanged
	}
	var pid uint32
	tid, _, err := windowProcess.Call(hwnd, uintptr(unsafe.Pointer(&pid)))
	if tid == 0 {
		return out, err
	}
	process, err := windows.OpenProcess(windows.PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
	if err != nil {
		return out, err
	}
	defer windows.CloseHandle(process)
	var created, exited, kernel, user windows.Filetime
	if err := windows.GetProcessTimes(process, &created, &exited, &kernel, &user); err != nil {
		return out, err
	}
	path := make([]uint16, 32768)
	length := uint32(len(path))
	if err := windows.QueryFullProcessImageName(process, 0, &path[0], &length); err != nil {
		return out, err
	}
	var class [256]uint16
	n, _, err := windowClass.Call(hwnd, uintptr(unsafe.Pointer(&class[0])), uintptr(len(class)))
	if n == 0 {
		return out, err
	}
	var title [1024]uint16
	windowTitle.Call(hwnd, uintptr(unsafe.Pointer(&title[0])), uintptr(len(title)))
	var finalPID uint32
	windowProcess.Call(hwnd, uintptr(unsafe.Pointer(&finalPID)))
	if finalPID != pid {
		return out, ErrIdentityChanged
	}
	return WindowIdentity{Handle: uint64(hwnd), ProcessID: pid, ProcessStartedAt: uint64(created.HighDateTime)<<32 | uint64(created.LowDateTime), Executable: windows.UTF16ToString(path[:length]), Class: windows.UTF16ToString(class[:]), Title: windows.UTF16ToString(title[:])}, nil
}
