//go:build windows && amd64

package desktop

import (
	"fmt"
	"golang.org/x/sys/windows"
	"unsafe"
)

var frameCounter = windows.NewLazySystemDLL("kernel32.dll").NewProc("QueryPerformanceCounter")
var frameFrequency = windows.NewLazySystemDLL("kernel32.dll").NewProc("QueryPerformanceFrequency")

func CaptureSystemTicks() (int64, error) {
	var counter, frequency int64
	ok, _, err := frameCounter.Call(uintptr(unsafe.Pointer(&counter)))
	if ok == 0 {
		return 0, fmt.Errorf("desktop.qpc: %w", err)
	}
	ok, _, err = frameFrequency.Call(uintptr(unsafe.Pointer(&frequency)))
	if ok == 0 {
		return 0, fmt.Errorf("desktop.qpc_frequency: %w", err)
	}
	return qpcToSystemTicks(counter, frequency)
}
