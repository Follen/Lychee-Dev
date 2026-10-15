//go:build windows

package resource

import (
	"unsafe"

	"golang.org/x/sys/windows"
)

var globalMemoryStatusEx = windows.NewLazySystemDLL("kernel32.dll").NewProc("GlobalMemoryStatusEx")

// MEMORYSTATUSEX: https://learn.microsoft.com/windows/win32/api/sysinfoapi/ns-sysinfoapi-memorystatusex
type memoryStatusEx struct {
	length, memoryLoad               uint32
	totalPhysical, availablePhysical uint64
	totalPageFile, availablePageFile uint64
	totalVirtual, availableVirtual   uint64
	availableExtendedVirtual         uint64
}

func availableMemory() (int64, error) {
	status := memoryStatusEx{}
	status.length = uint32(unsafe.Sizeof(status))
	ok, _, err := globalMemoryStatusEx.Call(uintptr(unsafe.Pointer(&status)))
	if ok == 0 {
		return 0, err
	}
	// AvailablePageFile is the calling process's remaining commit capacity,
	// which can be lower than the machine-wide value (for example in a job).
	return int64(min(status.availablePhysical, status.availablePageFile, status.availableVirtual)), nil
}

func newQueryBudget(concurrentQueries int) (*Budget, error) {
	return newAdaptive(DefaultLimits(), concurrentQueries, availableMemory)
}
