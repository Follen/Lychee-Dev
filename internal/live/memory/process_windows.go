//go:build windows && amd64

package memory

import (
	"bytes"
	"context"
	"errors"
	"strings"
	"unsafe"

	"golang.org/x/sys/windows"
)

type Process struct {
	handle  windows.Handle
	Created uint64
	Image   string
	PID     uint32
}

func Open(pid uint32, created uint64, image string) (*Process, error) {
	return openWithAccess(pid, created, image, windows.PROCESS_QUERY_INFORMATION|windows.PROCESS_VM_READ)
}

// OpenDuplexWriter is used only by the typed mailbox adapter. The default
// process API remains read-only; no command exposes arbitrary write addresses.
func OpenDuplexWriter(pid uint32, created uint64, image string) (*Process, error) {
	return openWithAccess(pid, created, image, windows.PROCESS_QUERY_INFORMATION|windows.PROCESS_VM_READ|windows.PROCESS_VM_WRITE|windows.PROCESS_VM_OPERATION)
}

func openWithAccess(pid uint32, created uint64, image string, access uint32) (*Process, error) {
	if pid == 0 || created == 0 || image == "" {
		return nil, errors.New("memory.process_identity_required")
	}
	h, err := windows.OpenProcess(access, false, pid)
	if err != nil {
		return nil, err
	}
	p := &Process{h, created, image, pid}
	if err = p.Verify(context.Background()); err != nil {
		p.Close()
		return nil, err
	}
	return p, nil
}

func (p *Process) WriteDuplexCell(ctx context.Context, cell NumericCell, value uint32, guard func(context.Context) error) (int, error) {
	if guard == nil {
		return 0, ErrReloadGuardRequired
	}
	if _, bounded := ctx.Deadline(); !bounded {
		return 0, errors.New("memory.write_deadline_required")
	}
	if err := p.Verify(ctx); err != nil {
		return 0, err
	}
	b := make([]byte, 10)
	if n, err := p.Read(ctx, cell.Address, b); err != nil || n != len(b) {
		return 0, errors.Join(mailboxError("numeric_cell_read"), err)
	}
	if b[8] != 3 || b[9] != 0 || !bytes.Equal(b[:8], numericPayload(cell.Value)) {
		return 0, mailboxError("numeric_cell_changed")
	}
	var info windows.MemoryBasicInformation
	if err := windows.VirtualQueryEx(p.handle, uintptr(cell.Address), &info, unsafe.Sizeof(info)); err != nil {
		return 0, err
	}
	start, size := uint64(info.BaseAddress), uint64(info.RegionSize)
	if cell.Address == 0 || cell.Address > ^uint64(0)-8 || info.Type != 0x20000 || info.State != windows.MEM_COMMIT || info.Protect != windows.PAGE_READWRITE || cell.Address < start || cell.Address-start > size || size-(cell.Address-start) < 8 {
		return 0, errors.New("memory.write_interval_ineligible")
	}
	if err := ctx.Err(); err != nil {
		return 0, err
	}
	// This must run after the cell and page checks for every individual write.
	// It blocks observed reload/teardown, but does not pin allocation lifetime.
	if err := guard(ctx); err != nil {
		return 0, err
	}
	if err := ctx.Err(); err != nil {
		return 0, err
	}
	payload := numericPayload(value)
	var n uintptr
	err := windows.WriteProcessMemory(p.handle, uintptr(cell.Address), &payload[0], 8, &n)
	return int(n), err
}
func (p *Process) Close() error { return windows.CloseHandle(p.handle) }
func (p *Process) Verify(ctx context.Context) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	var c, x, k, u windows.Filetime
	if err := windows.GetProcessTimes(p.handle, &c, &x, &k, &u); err != nil {
		return err
	}
	if uint64(c.HighDateTime)<<32|uint64(c.LowDateTime) != p.Created {
		return errors.New("memory.process_changed")
	}
	buf := make([]uint16, 32768)
	size := uint32(len(buf))
	if err := windows.QueryFullProcessImageName(p.handle, 0, &buf[0], &size); err != nil {
		return err
	}
	if !strings.EqualFold(windows.UTF16ToString(buf[:size]), p.Image) {
		return errors.New("memory.image_changed")
	}
	var code uint32
	if err := windows.GetExitCodeProcess(p.handle, &code); err != nil {
		return err
	}
	if code != 259 {
		return errors.New("memory.process_exited")
	}
	return nil
}
func readable(protect uint32) bool {
	if protect&windows.PAGE_GUARD != 0 {
		return false
	}
	switch protect & 255 {
	case 2, 4, 8, 32, 64, 128:
		return true
	}
	return false
}
func (p *Process) Regions(ctx context.Context) ([]Region, error) {
	var out []Region
	for address, count := uintptr(0), 0; count < 100000; count++ {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		var info windows.MemoryBasicInformation
		err := windows.VirtualQueryEx(p.handle, address, &info, unsafe.Sizeof(info))
		if err != nil {
			if errors.Is(err, windows.ERROR_INVALID_PARAMETER) && address > 0 {
				return out, nil
			}
			return nil, err
		}
		next := info.BaseAddress + info.RegionSize
		if next <= address {
			return nil, errors.New("memory.region_overflow")
		}
		out = append(out, Region{Range{uint64(info.BaseAddress), uint64(next)}, info.Type == 0x20000, info.State == windows.MEM_COMMIT, readable(info.Protect)})
		address = next
	}
	return nil, errors.New("memory.region_limit")
}
func (p *Process) Read(ctx context.Context, address uint64, b []byte) (int, error) {
	if err := ctx.Err(); err != nil {
		return 0, err
	}
	if len(b) == 0 {
		return 0, nil
	}
	if address+uint64(len(b)) < address {
		return 0, errors.New("memory.address_overflow")
	}
	// Point reads follow the same eligibility rules as scans, including every
	// adjacent mapping spanned by a record. No module or guard-page fallback.
	for at, end := address, address+uint64(len(b)); at < end; {
		var info windows.MemoryBasicInformation
		if err := windows.VirtualQueryEx(p.handle, uintptr(at), &info, unsafe.Sizeof(info)); err != nil {
			return 0, err
		}
		next := uint64(info.BaseAddress + info.RegionSize)
		if next <= at || info.Type != 0x20000 || info.State != windows.MEM_COMMIT || !readable(info.Protect) {
			return 0, errors.New("memory.ineligible_read")
		}
		at = next
	}
	var n uintptr
	err := windows.ReadProcessMemory(p.handle, uintptr(address), &b[0], uintptr(len(b)), &n)
	return int(n), err
}
