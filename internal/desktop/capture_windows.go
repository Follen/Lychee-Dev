//go:build windows && amd64

package desktop

import (
	"fmt"
	"runtime"
	"sync"
	"syscall"
	"unsafe"

	"golang.org/x/sys/windows"
)

// ABI slots and interface IDs are verified against Windows SDK 10.0.26100.0:
// windows.graphics.capture.h and windows.graphics.capture.interop.h.
var (
	runtimeLibrary      = windows.NewLazySystemDLL("combase.dll")
	initializeRuntime   = runtimeLibrary.NewProc("RoInitialize")
	finishRuntime       = runtimeLibrary.NewProc("RoUninitialize")
	createRuntimeString = runtimeLibrary.NewProc("WindowsCreateString")
	deleteRuntimeString = runtimeLibrary.NewProc("WindowsDeleteString")
	getRuntimeFactory   = runtimeLibrary.NewProc("RoGetActivationFactory")
	captureInteropID    = windows.GUID{Data1: 0x3628e81b, Data2: 0x3cac, Data3: 0x4c60, Data4: [8]byte{0xb7, 0xf4, 0x23, 0xce, 0x0e, 0x0c, 0x33, 0x56}}
	captureItemID       = windows.GUID{Data1: 0x79c3f95b, Data2: 0x31f7, Data3: 0x4ec2, Data4: [8]byte{0xa4, 0x64, 0x63, 0x2e, 0xf5, 0xd3, 0x07, 0x60}}
)

type nativeInterface struct{ vtable *[128]uintptr }

// WGC's free-threaded workers can finish after the capture objects are closed.
// If the last per-thread RoUninitialize tears down the process MTA in that gap,
// GraphicsCapture.dll can unload under a returning native worker. Retain one
// lazy process-lifetime MTA usage reference, not any capture/device objects.
// Do not decrement between streams: the OS reclaims this reference at process
// exit. Each capture still balances its own RoInitialize/RoUninitialize.
var retainCaptureRuntime = sync.OnceValue(func() error {
	var cookie uintptr
	retain := windows.NewLazySystemDLL("ole32.dll").NewProc("CoIncrementMTAUsage")
	code, _, _ := retain.Call(uintptr(unsafe.Pointer(&cookie)))
	return runtimeFailure("retain_capture_runtime", code)
})

// Pointer arguments travel through this helper before SyscallN. Keep their Go
// allocations alive and off moving stacks across the intervening call frame.
//
//go:uintptrescapes
func (n *nativeInterface) call(slot int, args ...uintptr) uintptr {
	parameters := make([]uintptr, 1, len(args)+1)
	parameters[0] = uintptr(unsafe.Pointer(n))
	parameters = append(parameters, args...)
	value, _, _ := syscall.SyscallN(n.vtable[slot], parameters...)
	runtime.KeepAlive(n)
	return value
}
func (n *nativeInterface) release() {
	if n != nil {
		n.call(2)
	}
}
func runtimeFailure(operation string, code uintptr) error {
	if int32(code) < 0 {
		return fmt.Errorf("desktop.%s: HRESULT 0x%08x", operation, uint32(code))
	}
	return nil
}

func activationFactory(class string, id *windows.GUID) (*nativeInterface, error) {
	text, err := windows.UTF16FromString(class)
	if err != nil {
		return nil, err
	}
	var handle uintptr
	code, _, _ := createRuntimeString.Call(uintptr(unsafe.Pointer(&text[0])), uintptr(len(text)-1), uintptr(unsafe.Pointer(&handle)))
	if err := runtimeFailure("runtime_string", code); err != nil {
		return nil, err
	}
	defer deleteRuntimeString.Call(handle)
	var factory *nativeInterface
	code, _, _ = getRuntimeFactory.Call(handle, uintptr(unsafe.Pointer(id)), uintptr(unsafe.Pointer(&factory)))
	if err := runtimeFailure("activation_factory", code); err != nil {
		return nil, err
	}
	return factory, nil
}

type captureExtent struct{ Width, Height int32 }

// openCaptureItem requires a caller-owned initialized WinRT apartment. The
// returned COM reference must be released before that apartment is shut down.
func openCaptureItem(hwnd uintptr) (*nativeInterface, captureExtent, error) {
	var size captureExtent
	factory, err := activationFactory("Windows.Graphics.Capture.GraphicsCaptureItem", &captureInteropID)
	if err != nil {
		return nil, size, err
	}
	defer factory.release()
	var item *nativeInterface
	if err := runtimeFailure("create_capture_item", factory.call(3, hwnd, uintptr(unsafe.Pointer(&captureItemID)), uintptr(unsafe.Pointer(&item)))); err != nil {
		return nil, size, err
	}
	if err := runtimeFailure("capture_size", item.call(7, uintptr(unsafe.Pointer(&size)))); err != nil {
		item.release()
		return nil, size, err
	}
	if size.Width <= 0 || size.Height <= 0 || size.Width > 16384 || size.Height > 16384 {
		item.release()
		return nil, size, fmt.Errorf("desktop.invalid_capture_size: %dx%d", size.Width, size.Height)
	}
	return item, size, nil
}
