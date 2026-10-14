//go:build windows && amd64

package desktop

import (
	"image"
	"runtime"
	"testing"
	"unsafe"

	"golang.org/x/sys/windows"
)

// Read only the test process, its window station, and the fixture HWND. These
// observations explain native failures; none turn a failure into a skip/pass
// or change window styles, desktop attachment, permissions, or capture policy.
func logWGCHostDiagnostics(t *testing.T, target WindowIdentity) {
	t.Helper()
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()
	hwnd := uintptr(target.Handle)
	var session uint32
	if err := windows.ProcessIdToSessionId(target.ProcessID, &session); err != nil {
		t.Logf("WGC diagnostic ProcessIdToSessionId: %v", err)
	} else {
		t.Logf("WGC diagnostic PID=%d session=%d HWND=%d", target.ProcessID, session, target.Handle)
	}

	info := userLibrary.NewProc("GetUserObjectInformationW")
	logObjectName := func(kind string, handle uintptr) {
		var name [256]uint16
		var needed uint32
		ok, _, err := info.Call(handle, 2, uintptr(unsafe.Pointer(&name[0])), unsafe.Sizeof(name), uintptr(unsafe.Pointer(&needed))) // UOI_NAME
		if ok == 0 {
			t.Logf("WGC diagnostic %s name: %v (required bytes=%d)", kind, err, needed)
		} else {
			t.Logf("WGC diagnostic %s=%q", kind, windows.UTF16ToString(name[:]))
		}
	}
	station, _, err := userLibrary.NewProc("GetProcessWindowStation").Call()
	if station == 0 {
		t.Logf("WGC diagnostic GetProcessWindowStation: %v", err)
	} else {
		logObjectName("window station", station)
		var flags struct {
			Inherit, Reserved int32
			Flags             uint32
		}
		ok, _, err := info.Call(station, 1, uintptr(unsafe.Pointer(&flags)), unsafe.Sizeof(flags), 0) // UOI_FLAGS
		if ok == 0 {
			t.Logf("WGC diagnostic station flags: %v", err)
		} else {
			t.Logf("WGC diagnostic station visible surfaces=%t (flags=%#x)", flags.Flags&1 != 0, flags.Flags) // WSF_VISIBLE
		}
	}
	thread, _, err := windowProcess.Call(hwnd, 0)
	if thread == 0 {
		t.Logf("WGC diagnostic fixture thread: %v", err)
	} else {
		desktop, _, err := userLibrary.NewProc("GetThreadDesktop").Call(thread)
		if desktop == 0 {
			t.Logf("WGC diagnostic GetThreadDesktop: %v", err)
		} else {
			logObjectName("fixture desktop", desktop)
			var input int32
			ok, _, err := info.Call(desktop, 6, uintptr(unsafe.Pointer(&input)), unsafe.Sizeof(input), 0) // UOI_IO
			if ok == 0 {
				t.Logf("WGC diagnostic desktop input state: %v", err)
			} else {
				t.Logf("WGC diagnostic fixture desktop receives input=%t", input != 0)
			}
		}
	}

	visible, _, _ := windowVisible.Call(hwnd)
	minimized, _, _ := windowMinimized.Call(hwnd)
	style, _, _ := userLibrary.NewProc("GetWindowLongPtrW").Call(hwnd, ^uintptr(15))    // GWL_STYLE
	extended, _, _ := userLibrary.NewProc("GetWindowLongPtrW").Call(hwnd, ^uintptr(19)) // GWL_EXSTYLE
	t.Logf("WGC diagnostic fixture visible=%t minimized=%t style=%#x exstyle=%#x", visible != 0, minimized != 0, uint32(style), uint32(extended))
	for _, name := range []string{"GetWindowRect", "GetClientRect"} {
		var rect struct{ Left, Top, Right, Bottom int32 }
		ok, _, err := userLibrary.NewProc(name).Call(hwnd, uintptr(unsafe.Pointer(&rect)))
		if ok == 0 {
			t.Logf("WGC diagnostic %s: %v", name, err)
		} else {
			t.Logf("WGC diagnostic %s=%+v", name, rect)
		}
	}
	dwm := windows.NewLazySystemDLL("dwmapi.dll")
	var composition int32
	code, _, _ := dwm.NewProc("DwmIsCompositionEnabled").Call(uintptr(unsafe.Pointer(&composition)))
	// This API's boolean can depend on the executable's compatibility manifest;
	// it is not, by itself, proof that the host lacks a compositor.
	t.Logf("WGC diagnostic DwmIsCompositionEnabled=%t HRESULT=%#08x (manifest-sensitive)", composition != 0, uint32(code))
	var cloaked uint32
	code, _, _ = dwm.NewProc("DwmGetWindowAttribute").Call(hwnd, 14, uintptr(unsafe.Pointer(&cloaked)), unsafe.Sizeof(cloaked)) // DWMWA_CLOAKED
	t.Logf("WGC diagnostic fixture cloaked=%#x HRESULT=%#08x", cloaked, uint32(code))
	var affinity uint32
	ok, _, err := userLibrary.NewProc("GetWindowDisplayAffinity").Call(hwnd, uintptr(unsafe.Pointer(&affinity)))
	if ok == 0 {
		t.Logf("WGC diagnostic GetWindowDisplayAffinity: %v", err)
	} else {
		t.Logf("WGC diagnostic fixture display affinity=%#x", affinity)
	}

	code, _, _ = initializeRuntime.Call(1)
	if err := runtimeFailure("diagnostic_initialize_runtime", code); err != nil {
		t.Log(err)
		return
	}
	defer finishRuntime.Call()
	// Windows SDK: IGraphicsCaptureSessionStatics derives from IInspectable;
	// IsSupported is slot 6 and returns a one-byte WinRT boolean, not a BOOL.
	staticsID := guid("2224a540-5974-49aa-b232-0882536f4cb5")
	statics, err := activationFactory("Windows.Graphics.Capture.GraphicsCaptureSession", &staticsID)
	if err != nil {
		t.Logf("WGC diagnostic GraphicsCaptureSession factory: %v", err)
		return
	}
	defer statics.release()
	var supported uint8
	code = statics.call(6, uintptr(unsafe.Pointer(&supported)))
	t.Logf("WGC diagnostic GraphicsCaptureSession.IsSupported=%t HRESULT=%#08x", supported != 0, uint32(code))
}

func logWGCCaptureItemControls(t *testing.T, original WindowIdentity) {
	t.Helper()
	// The original capture already failed. A later item-only retry and a second
	// test-owned HWND distinguish transient startup from a possible style issue;
	// neither is a replacement for the failed lifetime assertion.
	control := openFixtureWindowWithStyle(t, image.NewNRGBA(image.Rect(0, 0, 160, 120)), 0x00cf0000, 0x08000000) // WS_OVERLAPPEDWINDOW; WS_EX_NOACTIVATE
	logWGCHostDiagnostics(t, control.identity)
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()
	code, _, _ := initializeRuntime.Call(1)
	if err := runtimeFailure("diagnostic_initialize_runtime", code); err != nil {
		t.Log(err)
		return
	}
	defer finishRuntime.Call()
	for _, target := range []struct {
		name     string
		identity WindowIdentity
	}{{"original fixture, later item-only attempt", original}, {"unowned overlapped control, item-only attempt", control.identity}} {
		item, size, err := openCaptureItem(uintptr(target.identity.Handle))
		if err != nil {
			t.Logf("WGC diagnostic %s HWND=%d: %v", target.name, target.identity.Handle, err)
		} else {
			item.release()
			t.Logf("WGC diagnostic %s HWND=%d extent=%dx%d (does not validate frame capture or lifetime)", target.name, target.identity.Handle, size.Width, size.Height)
		}
	}
}
