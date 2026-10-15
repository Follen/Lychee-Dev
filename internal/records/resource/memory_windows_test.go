//go:build windows

package resource

import (
	"testing"
	"unsafe"
)

func TestWindowsQueryMemoryUsesNativeAvailability(t *testing.T) {
	if size := unsafe.Sizeof(memoryStatusEx{}); size != 64 {
		t.Fatalf("MEMORYSTATUSEX ABI size=%d", size)
	}
	available, err := availableMemory()
	if err != nil || available <= 0 {
		t.Fatalf("available memory=%d err=%v", available, err)
	}
	b, err := NewQuery(2)
	if err != nil {
		t.Fatal(err)
	}
	if got := b.MemorySnapshot(); got.Mode != "windows_adaptive" || got.ConcurrentQueries != 2 || got.CeilingBytes <= 0 {
		t.Fatal(got)
	}
	t.Logf("native query budget: %+v", b.MemorySnapshot())
}
