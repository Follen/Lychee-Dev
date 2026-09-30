//go:build windows && amd64

package memory

import (
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
	if pid == 0 || created == 0 || image == "" {
		return nil, errors.New("memory.process_identity_required")
	}
	h, err := windows.OpenProcess(windows.PROCESS_QUERY_INFORMATION|windows.PROCESS_VM_READ, false, pid)
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
	return p.regionsMeasured(ctx, nil)
}
func (p *Process) regionsMeasured(ctx context.Context, session *Session) ([]Region, error) {
	var out []Region
	for address, count := uintptr(0), 0; count < 100000; count++ {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		var info windows.MemoryBasicInformation
		if session != nil {
			session.update(func(st *Stats) { st.MappingQueries++ })
		}
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
	return p.readMeasured(ctx, address, b, nil)
}
func (p *Process) readMeasured(ctx context.Context, address uint64, b []byte, session *Session) (int, error) {
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
		if session != nil {
			session.update(func(st *Stats) { st.MappingQueries++ })
		}
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
	if session != nil {
		session.update(func(st *Stats) { st.RPMCalls++ })
	}
	err := windows.ReadProcessMemory(p.handle, uintptr(address), &b[0], uintptr(len(b)), &n)
	return int(n), err
}
