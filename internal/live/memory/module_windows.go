//go:build windows && amd64

package memory

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"io"
	"os"
	"sort"
	"strings"
	"unsafe"

	"golang.org/x/sys/windows"
)

const moduleImageType = 0x1000000 // MEM_IMAGE
const moduleMappedType = 0x40000  // MEM_MAPPED; neither exception admits MEM_PRIVATE.
const moduleInventoryLimit = 4096
const moduleSnapshotAttempts = 4
const moduleExecutableLimit int64 = 512 << 20

// The OS module range is independently bounded, even for a small executable
// with a large virtual image. One mapping per system page is the worst case.
const moduleVirtualLimit uint64 = 512 << 20
const moduleReadLimit = 1 << 20

// Module is an OS-inventoried main executable, bound to one Process instance.
// The executable digest is computed at initialization, never during root reads.
// Public metadata alone cannot grant access to an arbitrary image/mapped page.
type Module struct {
	Name             string
	Base, Size       uint64
	ExecutableSHA256 string
	proof            *moduleProof
}

type moduleProof struct {
	process     *Process
	created     uint64
	pid         uint32
	image, name string
	base, size  uint64
	digest      string
	layout      LuaRootImageLayout
	segments    []moduleSegment
}

// MainModule obtains ASLR relocation and size from the Windows loader inventory.
// Its executable layout is parsed from the same locked file used for SHA256;
// it never scans process memory for a locator.
func (p *Process) MainModule(ctx context.Context) (Module, error) {
	if err := p.Verify(ctx); err != nil {
		return Module{}, err
	}
	var snapshot windows.Handle
	var err error
	for attempt := 0; attempt < moduleSnapshotAttempts; attempt++ {
		if err := ctx.Err(); err != nil {
			return Module{}, err
		}
		snapshot, err = windows.CreateToolhelp32Snapshot(windows.TH32CS_SNAPMODULE, p.PID)
		if !errors.Is(err, windows.ERROR_BAD_LENGTH) {
			break
		}
	}
	if err != nil {
		return Module{}, err
	}
	defer windows.CloseHandle(snapshot)
	entry := windows.ModuleEntry32{Size: uint32(windows.SizeofModuleEntry32)}
	if err := windows.Module32First(snapshot, &entry); err != nil {
		return Module{}, err
	}
	for count := 0; count < moduleInventoryLimit; count++ {
		if err := ctx.Err(); err != nil {
			return Module{}, err
		}
		image := windows.UTF16ToString(entry.ExePath[:])
		if strings.EqualFold(image, p.Image) {
			base, size := uint64(entry.ModBaseAddr), uint64(entry.ModBaseSize)
			if entry.ProcessID != p.PID || base == 0 || size == 0 || base+size < base {
				return Module{}, errors.New("memory.module_identity_invalid")
			}
			if err := p.Verify(ctx); err != nil {
				return Module{}, err
			}
			digest, layout, err := hashModuleExecutable(ctx, image, size)
			if err != nil {
				return Module{}, err
			}
			if err := p.Verify(ctx); err != nil {
				return Module{}, err
			}
			segments, err := captureModuleSegments(ctx, base, size, p.queryModuleMapping)
			if err != nil {
				return Module{}, err
			}
			if err := p.Verify(ctx); err != nil {
				return Module{}, err
			}
			if err := ctx.Err(); err != nil {
				return Module{}, err
			}
			name := windows.UTF16ToString(entry.Module[:])
			proof := &moduleProof{process: p, created: p.Created, pid: p.PID, image: image, name: name, base: base, size: size, digest: digest, layout: layout, segments: segments}
			return Module{Name: name, Base: base, Size: size, ExecutableSHA256: digest, proof: proof}, nil
		}
		if err := windows.Module32Next(snapshot, &entry); err != nil {
			if errors.Is(err, windows.ERROR_NO_MORE_FILES) {
				return Module{}, errors.New("memory.main_module_missing")
			}
			return Module{}, err
		}
	}
	return Module{}, errors.New("memory.module_inventory_limit")
}

func hashModuleExecutable(ctx context.Context, image string, moduleSize uint64) (string, LuaRootImageLayout, error) {
	var empty LuaRootImageLayout
	if err := ctx.Err(); err != nil {
		return "", empty, err
	}
	path, err := windows.UTF16PtrFromString(image)
	if err != nil {
		return "", empty, err
	}
	// One read-sharing-only handle excludes replacement/modification throughout
	// both hashing and layout parsing. ReaderAt does not change the hash cursor.
	handle, err := windows.CreateFile(path, windows.GENERIC_READ, windows.FILE_SHARE_READ, nil, windows.OPEN_EXISTING, windows.FILE_ATTRIBUTE_NORMAL, 0)
	if err != nil {
		return "", empty, err
	}
	file := os.NewFile(uintptr(handle), image)
	defer file.Close()
	info, err := file.Stat()
	if err != nil {
		return "", empty, err
	}
	if !info.Mode().IsRegular() || info.Size() <= 0 || info.Size() > moduleExecutableLimit {
		return "", empty, errors.New("memory.module_executable_limit")
	}
	hash := sha256.New()
	buffer := make([]byte, 1<<20)
	var total int64
	for {
		if err := ctx.Err(); err != nil {
			return "", empty, err
		}
		n, err := file.Read(buffer)
		if n > 0 {
			total += int64(n)
			if total > moduleExecutableLimit {
				return "", empty, errors.New("memory.module_executable_limit")
			}
			_, _ = hash.Write(buffer[:n])
		}
		if errors.Is(err, io.EOF) {
			break
		}
		if err != nil {
			return "", empty, err
		}
		if n == 0 {
			return "", empty, io.ErrNoProgress
		}
	}
	if total != info.Size() {
		return "", empty, errors.New("memory.module_executable_changed")
	}
	if err := ctx.Err(); err != nil {
		return "", empty, err
	}
	digest := hex.EncodeToString(hash.Sum(nil))
	layout, err := readLuaRootImageLayout(file, digest, moduleSize)
	if err != nil {
		return "", empty, err
	}
	if !layout.validFor(digest, moduleSize) {
		return "", empty, errors.New("memory.module_layout_identity_invalid")
	}
	if err := ctx.Err(); err != nil {
		return "", empty, err
	}
	return digest, layout, nil
}

func (m Module) validFor(p *Process) bool {
	f := m.proof
	return f != nil && p != nil && f.layout.validFor(m.ExecutableSHA256, m.Size) && len(f.segments) > 0 && f.process == p && f.created == p.Created && f.pid == p.PID && strings.EqualFold(f.image, p.Image) && m.Name == f.name && m.Base == f.base && m.Size == f.size && m.ExecutableSHA256 == f.digest
}

// LuaImageLayout returns only the opaque layout issued by the locked file
// verification. Caller-supplied Module metadata cannot manufacture a layout.
func (m Module) LuaImageLayout() LuaRootImageLayout {
	if m.proof == nil || !m.validFor(m.proof.process) {
		return LuaRootImageLayout{}
	}
	return m.proof.layout
}

// The issued module-clamped spans and mapping identity remain sealed.
// A later OS run may split outside a read without changing its authorization.
// Ineligible holes remain part of the plan but can never grant read authority.
type moduleSegment struct {
	start, end, regionStart, regionEnd, allocation uint64
	kind                                           uint32
	state, protect                                 uint32
	eligible                                       bool
}
type moduleMappingQuery func(uint64) (windows.MemoryBasicInformation, error)

func moduleMappingEnd(info windows.MemoryBasicInformation, at, moduleBase, moduleEnd uint64) (uint64, bool) {
	start, size, allocation := uint64(info.BaseAddress), uint64(info.RegionSize), uint64(info.AllocationBase)
	end := start + size
	geometry := size != 0 && end > start && start <= at && end > at && at >= moduleBase && at < moduleEnd
	if !geometry {
		return end, false
	}
	if end > moduleEnd {
		end = moduleEnd
	}
	eligible := (info.Type == moduleImageType || info.Type == moduleMappedType) && info.State == windows.MEM_COMMIT && readable(info.Protect) && allocation >= moduleBase && allocation < moduleEnd && allocation <= start
	return end, eligible
}

func captureModuleSegments(ctx context.Context, base, size uint64, query moduleMappingQuery) ([]moduleSegment, error) {
	end := base + size
	page := uint64(os.Getpagesize())
	limit, valid := moduleMappingCapacity(base, size, page)
	if !valid || query == nil {
		return nil, errors.New("memory.module_mapping_range")
	}
	segments := make([]moduleSegment, 0, 16)
	for at := base; at < end; {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		if uint64(len(segments)) >= limit {
			return nil, errors.New("memory.module_mapping_limit")
		}
		info, err := query(at)
		if err != nil {
			return nil, err
		}
		rawStart, rawSize := uint64(info.BaseAddress), uint64(info.RegionSize)
		rawEnd := rawStart + rawSize
		if rawSize == 0 || rawEnd <= rawStart || rawStart > at || rawEnd <= at || rawStart%page != 0 || rawSize%page != 0 {
			return nil, errors.New("memory.module_mapping_geometry")
		}
		next, eligible := moduleMappingEnd(info, at, base, end)
		segments = append(segments, moduleSegment{start: at, end: next, regionStart: rawStart, regionEnd: rawEnd, allocation: uint64(info.AllocationBase), kind: info.Type, state: info.State, protect: info.Protect, eligible: eligible})
		at = next
	}
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	return segments, nil
}

func moduleMappingCapacity(base, size, page uint64) (uint64, bool) {
	if page == 0 || base == 0 || size == 0 || size > moduleVirtualLimit || base+size <= base || base%page != 0 || size%page != 0 {
		return 0, false
	}
	return size / page, true
}

func verifyModuleSegments(ctx context.Context, base, size, address, end uint64, segments []moduleSegment, query moduleMappingQuery) error {
	moduleEnd := base + size
	page := uint64(os.Getpagesize())
	limit, valid := moduleMappingCapacity(base, size, page)
	if !valid || address < base || end < address || end > moduleEnd || len(segments) == 0 || uint64(len(segments)) > limit || query == nil {
		return moduleMappingDiagnostic("invalid_request", address, base, moduleEnd, nil, nil)
	}
	for at := address; at < end; {
		if err := ctx.Err(); err != nil {
			return err
		}
		var issued *moduleSegment
		// Capture seals an ordered, non-overlapping plan. Search only that
		// issued plan; never query an arbitrary address to fill a missing span.
		i := sort.Search(len(segments), func(i int) bool { return segments[i].end > at })
		if i < len(segments) && segments[i].start <= at {
			issued = &segments[i]
		}
		if issued == nil {
			return moduleMappingDiagnostic("issued_missing", at, base, moduleEnd, nil, nil)
		}
		if !issued.eligible {
			return moduleMappingDiagnostic("issued_ineligible", at, base, moduleEnd, issued, nil)
		}
		// The old OS run origin is not an allocation identity. Query the
		// actual requested location so unrelated preceding splits do not
		// describe a different page instead of the bytes being authorized.
		info, err := query(at)
		if err != nil {
			return err
		}
		next, eligible := moduleMappingEnd(info, at, base, moduleEnd)
		aligned := uint64(info.BaseAddress)%page == 0 && uint64(info.RegionSize)%page == 0
		if !aligned || !eligible || uint64(info.AllocationBase) != issued.allocation || info.Type != issued.kind || info.State != issued.state || info.Protect != issued.protect {
			reason := "current_coverage"
			switch {
			case !aligned:
				reason = "current_geometry"
			case !eligible:
				reason = moduleMappingIneligibleReason(info, at, base, moduleEnd)
			case uint64(info.AllocationBase) != issued.allocation:
				reason = "changed_allocation"
			case info.Type != issued.kind:
				reason = "changed_type"
			case info.State != issued.state:
				reason = "changed_state"
			case info.Protect != issued.protect:
				reason = "changed_protection"
			}
			return moduleMappingDiagnostic(reason, at, base, moduleEnd, issued, &info)
		}
		// Intersect each current piece with the original authorization plan.
		// Both current splits and issued boundaries require fresh checks; an
		// expanded run never fills a hole or skips an issued identity gate.
		at = min(end, issued.end, next)
	}
	return ctx.Err()
}

// Diagnostics describe only already-observed module mappings. They perform no
// reads, retain no memory contents, and do not change the eligibility predicate.
func moduleMappingIneligibleReason(info windows.MemoryBasicInformation, at, base, end uint64) string {
	start, size := uint64(info.BaseAddress), uint64(info.RegionSize)
	if size == 0 || start+size <= start || start > at || start+size <= at || at < base || at >= end {
		return "current_geometry"
	}
	if info.Type != moduleImageType && info.Type != moduleMappedType {
		return "current_type"
	}
	if info.State != windows.MEM_COMMIT {
		return "current_state"
	}
	if !readable(info.Protect) {
		return "current_protection"
	}
	return "current_allocation"
}

func moduleMappingDiagnostic(reason string, address, base, end uint64, issued *moduleSegment, current *windows.MemoryBasicInformation) error {
	issuedText := "missing"
	if issued != nil {
		issuedText = fmt.Sprintf("span=[%#x,%#x) region=[%#x,%#x) type=%#x allocation=%#x state=%#x protect=%#x eligible=%t", issued.start, issued.end, issued.regionStart, issued.regionEnd, issued.kind, issued.allocation, issued.state, issued.protect, issued.eligible)
	}
	currentText := "not_queried"
	if current != nil {
		currentText = fmt.Sprintf("region=[%#x,%#x) size=%#x type=%#x allocation=%#x state=%#x protect=%#x", current.BaseAddress, uint64(current.BaseAddress)+uint64(current.RegionSize), current.RegionSize, current.Type, current.AllocationBase, current.State, current.Protect)
	}
	return fmt.Errorf("memory.ineligible_module_read reason=%s address=%#x module=[%#x,%#x) issued={%s} current={%s}", reason, address, base, end, issuedText, currentText)
}

func (p *Process) queryModuleMapping(at uint64) (windows.MemoryBasicInformation, error) {
	var info windows.MemoryBasicInformation
	err := windows.VirtualQueryEx(p.handle, uintptr(at), &info, unsafe.Sizeof(info))
	return info, err
}

// ReadModule is the sole image/mapped-page exception, restricted to the sealed
// segments of an OS-issued main module. Ordinary Process.Read/Regions remain
// private-only. A caller's common
// observation Source must account for these reads in its shared physical budget.
// Windows calls may overrun a context; checks surround them without retry loops.
func (p *Process) ReadModule(ctx context.Context, module Module, address uint64, b []byte) (int, error) {
	if err := ctx.Err(); err != nil {
		return 0, err
	}
	if !module.validFor(p) {
		return 0, errors.New("memory.module_identity_changed")
	}
	if len(b) > moduleReadLimit {
		return 0, errors.New("memory.module_read_limit")
	}
	end := address + uint64(len(b))
	if end < address || address < module.Base || end > module.Base+module.Size {
		return 0, errors.New("memory.module_read_range")
	}
	if err := p.Verify(ctx); err != nil {
		return 0, err
	}
	if len(b) == 0 {
		return 0, ctx.Err()
	}
	if err := verifyModuleSegments(ctx, module.Base, module.Size, address, end, module.proof.segments, p.queryModuleMapping); err != nil {
		return 0, fmt.Errorf("memory.module_mapping_check stage=before_read: %w", err)
	}
	var n uintptr
	readErr := windows.ReadProcessMemory(p.handle, uintptr(address), &b[0], uintptr(len(b)), &n)
	if err := verifyModuleSegments(ctx, module.Base, module.Size, address, end, module.proof.segments, p.queryModuleMapping); err != nil {
		return 0, errors.Join(readErr, fmt.Errorf("memory.module_mapping_check stage=after_read: %w", err))
	}
	if err := p.Verify(ctx); err != nil {
		return 0, errors.Join(readErr, err)
	}
	if err := ctx.Err(); err != nil {
		return 0, errors.Join(readErr, err)
	}
	return int(n), readErr
}
