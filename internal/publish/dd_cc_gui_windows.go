//go:build windows && amd64

package publish

import (
	"context"
	"golang.org/x/sys/windows"
	"path/filepath"
	"strings"
	"unsafe"
)

func (d *ddDesktop) channels(ctx context.Context) (any, error) {
	if err := d.verify(ctx); err != nil {
		return nil, err
	}
	ccid := filepath.Base(filepath.Dir(d.profile))
	if !ddDecimal(ccid) {
		return nil, ddManagedError()
	}
	for _, resource := range [][2]string{{"netease_dd.exe", "4d6609e58d639f8ea6c513d3806904ae60896251ffbd4d9bc0e2361779dd947f"}, {"python37.dll", "268af469cc5b487d32848142d17f3640adc276bd85935e602cc9a295f8d7703d"}, {"ccvoicehub.res", "953b1bd893adb53c8b003d490ee219fe4a0e075efbed960fc8bc1bcff2a26892"}} {
		f, err := ddPinSDK(filepath.Join(d.installation, resource[0]), resource[1])
		if err != nil {
			return nil, err
		}
		defer f.Close()
	}
	process, err := windows.OpenProcess(windows.PROCESS_QUERY_LIMITED_INFORMATION|windows.PROCESS_VM_READ, false, uint32(d.guiPID))
	if err != nil {
		if err == windows.ERROR_ACCESS_DENIED {
			return nil, capability("publish.dd_channel_memory_denied", "the original DD GUI requires the same Windows integrity level for its read-only channel query")
		}
		return nil, ddManagedError()
	}
	defer windows.CloseHandle(process)
	var created, exit, kernel, user windows.Filetime
	if windows.GetProcessTimes(process, &created, &exit, &kernel, &user) != nil || uint64(created.HighDateTime)<<32|uint64(created.LowDateTime) != d.guiCreated {
		return nil, ddManagedError()
	}
	snapshot, err := windows.CreateToolhelp32Snapshot(windows.TH32CS_SNAPMODULE|windows.TH32CS_SNAPMODULE32, uint32(d.guiPID))
	if err != nil {
		return nil, ddManagedError()
	}
	defer windows.CloseHandle(snapshot)
	entry := windows.ModuleEntry32{Size: uint32(unsafe.Sizeof(windows.ModuleEntry32{}))}
	if windows.Module32First(snapshot, &entry) != nil {
		return nil, ddManagedError()
	}
	base := uint64(0)
	expected := filepath.Join(d.installation, "python37.dll")
	for {
		if strings.EqualFold(filepath.Clean(windows.UTF16ToString(entry.ExePath[:])), expected) {
			if uint64(entry.ModBaseSize) < ddPythonRuntimeRVA+440 {
				return nil, ddManagedError()
			}
			base = uint64(entry.ModBaseAddr)
			break
		}
		if windows.Module32Next(snapshot, &entry) != nil {
			break
		}
	}
	if base == 0 {
		return nil, ddManagedError()
	}
	reader := &ddManagedReader{ctx: ctx, read: func(address uint64, size int) ([]byte, error) {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		buf := make([]byte, size)
		var n uintptr
		if windows.ReadProcessMemory(process, uintptr(address), &buf[0], uintptr(size), &n) != nil || n != uintptr(size) {
			return nil, ddManagedError()
		}
		return buf, nil
	}}
	return ddGUIChannelsWith(ctx, reader, base, ccid, NewHTTP(), ddCCOrigin, d.verify)
}
