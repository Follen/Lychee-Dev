//go:build windows && amd64

package desktop

import (
	"os"
	"path/filepath"
	"testing"
	"unsafe"

	"golang.org/x/sys/windows"
)

// Mirrors testdata/wgc_reference.cpp. The SDK adapter validates this exact
// size and its own process ownership before touching a supplied fixture HWND.
type wgcReferenceResult struct {
	Version, Factory, Query, ClearError, Create, Extent uint32
	Width, Height                                       int32
	ItemID                                              windows.GUID
	ErrorInfo, ErrorDetails, ReportedCode, HasErrorInfo uint32
	DescriptionChars, RestrictedChars, CapabilityChars  uint32
	Description                                         [512]uint16
	Restricted                                          [1024]uint16
	CapabilitySID                                       [256]uint16
}

func TestWGCReferenceResultLayout(t *testing.T) {
	var result wgcReferenceResult
	if unsafe.Sizeof(result) != 3660 || unsafe.Offsetof(result.ItemID) != 32 || unsafe.Offsetof(result.Description) != 76 {
		t.Fatalf("SDK diagnostic result ABI drift: size=%d IID offset=%d text offset=%d", unsafe.Sizeof(result), unsafe.Offsetof(result.ItemID), unsafe.Offsetof(result.Description))
	}
}

func loadWGCReference(t *testing.T) *windows.Proc {
	t.Helper()
	path := os.Getenv("LYCHEEDEV_TEST_WGC_REFERENCE_DLL")
	if path == "" {
		t.Log("WGC diagnostic SDK reference DLL not configured")
		return nil
	}
	if !filepath.IsAbs(path) {
		t.Errorf("WGC diagnostic SDK reference DLL must have an absolute path: %q", path)
		return nil
	}
	library, err := windows.LoadDLL(path)
	if err != nil {
		t.Errorf("WGC diagnostic SDK reference DLL: %v", err)
		return nil
	}
	t.Cleanup(func() {
		if err := library.Release(); err != nil {
			t.Errorf("WGC diagnostic SDK reference DLL release: %v", err)
		}
	})
	proc, err := library.FindProc("LycheeWGCReference")
	if err != nil {
		t.Errorf("WGC diagnostic SDK reference export: %v", err)
		return nil
	}
	return proc
}

// The caller owns a locked, initialized MTA. Reproduce the exact production
// factory/call boundary on the fixture, retaining error info before Release.
// This is a later comparison attempt, not the initial lifetime failure.
func logWGCCaptureItemComparison(t *testing.T, name string, target WindowIdentity, reference *windows.Proc) {
	t.Helper()
	hwnd := uintptr(target.Handle)
	factory, err := activationFactory("Windows.Graphics.Capture.GraphicsCaptureItem", &captureInteropID)
	if err != nil {
		t.Logf("WGC diagnostic Go %s factory: %v", name, err)
		return
	}
	defer factory.release()
	var previous *nativeInterface
	clear, _, _ := runtimeLibrary.NewProc("GetRestrictedErrorInfo").Call(uintptr(unsafe.Pointer(&previous)))
	previous.release() // Retrieval clears any stale thread-local error object.
	var item *nativeInterface
	code := factory.call(3, hwnd, uintptr(unsafe.Pointer(&captureItemID)), uintptr(unsafe.Pointer(&item)))
	// No other COM operation may intervene before this thread-local retrieval.
	var details wgcRestrictedError
	if int32(code) < 0 {
		details = readWGCRestrictedError()
	}
	t.Logf("WGC diagnostic Go %s HWND=%d clear-error HRESULT=%#08x CreateForWindow HRESULT=%#08x", name, target.Handle, uint32(clear), uint32(code))
	if int32(code) < 0 {
		t.Logf("WGC diagnostic Go restricted error: retrieval=%#08x present=%t details=%#08x reported=%#08x description=%q restricted=%q capability=%q", details.Retrieval, details.Present, details.Details, details.ReportedCode, details.Description, details.Restricted, details.CapabilitySID)
	} else if item != nil {
		var size captureExtent
		sizeCode := item.call(7, uintptr(unsafe.Pointer(&size)))
		t.Logf("WGC diagnostic Go item extent=%dx%d HRESULT=%#08x (item-only)", size.Width, size.Height, uint32(sizeCode))
	}
	item.release()
	if reference == nil {
		return
	}
	var result wgcReferenceResult
	code, _, _ = reference.Call(hwnd, uintptr(unsafe.Pointer(&result)), unsafe.Sizeof(result))
	t.Logf("WGC diagnostic SDK %s HWND=%d result HRESULT=%#08x version=%d factory=%#08x query=%#08x clear-error=%#08x create=%#08x extent=%#08x size=%dx%d IID=%s", name, target.Handle, uint32(code), result.Version, result.Factory, result.Query, result.ClearError, result.Create, result.Extent, result.Width, result.Height, result.ItemID.String())
	t.Logf("WGC diagnostic SDK restricted error: retrieval=%#08x present=%t details=%#08x reported=%#08x description=%q (%d chars) restricted=%q (%d chars) capability=%q (%d chars)", result.ErrorInfo, result.HasErrorInfo != 0, result.ErrorDetails, result.ReportedCode, windows.UTF16ToString(result.Description[:]), result.DescriptionChars, windows.UTF16ToString(result.Restricted[:]), result.RestrictedChars, windows.UTF16ToString(result.CapabilitySID[:]), result.CapabilityChars)
	if result.Version != 1 || result.ItemID != captureItemID {
		t.Errorf("WGC diagnostic SDK result identity mismatch: version=%d IID=%s", result.Version, result.ItemID.String())
	}
}

type wgcRestrictedError struct {
	Retrieval, Details, ReportedCode       uint32
	Present                                bool
	Description, Restricted, CapabilitySID string
}

func readWGCRestrictedError() wgcRestrictedError {
	result := wgcRestrictedError{Details: 0x8000000a, ReportedCode: 0x8000000a} // E_PENDING until queried.
	var info *nativeInterface
	code, _, _ := runtimeLibrary.NewProc("GetRestrictedErrorInfo").Call(uintptr(unsafe.Pointer(&info)))
	result.Retrieval = uint32(code)
	result.Present = info != nil
	if info == nil {
		return result
	}
	defer info.release()
	var description, restricted, capability *uint16
	code = info.call(3, uintptr(unsafe.Pointer(&description)), uintptr(unsafe.Pointer(&result.ReportedCode)), uintptr(unsafe.Pointer(&restricted)), uintptr(unsafe.Pointer(&capability)))
	result.Details = uint32(code)
	automation := windows.NewLazySystemDLL("oleaut32.dll")
	read := func(value *uint16) string {
		if value == nil {
			return ""
		}
		defer automation.NewProc("SysFreeString").Call(uintptr(unsafe.Pointer(value)))
		length, _, _ := automation.NewProc("SysStringLen").Call(uintptr(unsafe.Pointer(value)))
		return windows.UTF16ToString(unsafe.Slice(value, int(length)))
	}
	result.Description, result.Restricted, result.CapabilitySID = read(description), read(restricted), read(capability)
	return result
}
