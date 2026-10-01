//go:build windows && amd64

package memory

import (
	"bufio"
	"bytes"
	"context"
	"crypto/sha256"
	"debug/pe"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"os"
	"os/exec"
	"strings"
	"syscall"
	"testing"
	"time"

	"golang.org/x/sys/windows"
)

func TestMainModuleChildBoundariesAndIdentity(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()
	command := exec.CommandContext(ctx, os.Args[0], "-test.run=^TestNativeMemoryChild$")
	command.Env = append(os.Environ(), "LYCHEE_MEMORY_CHILD=1")
	command.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}
	input, err := command.StdinPipe()
	if err != nil {
		t.Fatal(err)
	}
	output, err := command.StdoutPipe()
	if err != nil {
		t.Fatal(err)
	}
	var diagnostics bytes.Buffer
	command.Stderr = &diagnostics
	if err = command.Start(); err != nil {
		t.Fatal(err)
	}
	waited := false
	defer func() {
		input.Close()
		if !waited {
			if err := command.Wait(); err != nil {
				t.Errorf("child: %v %s", err, diagnostics.String())
			}
		}
	}()
	var child childMemory
	if err := json.NewDecoder(output).Decode(&child); err != nil {
		t.Fatal(err)
	}
	p, err := Open(child.PID, child.Created, child.Image)
	if err != nil {
		t.Fatal(err)
	}
	defer p.Close()
	m, err := p.MainModule(ctx)
	if err != nil {
		t.Fatal(err)
	}
	if m.Base == 0 || m.Size == 0 || m.Name == "" || !m.LuaImageLayout().validFor(m.ExecutableSHA256, m.Size) {
		t.Fatal("missing OS module metadata")
	}
	file, err := os.ReadFile(child.Image)
	if err != nil {
		t.Fatal(err)
	}
	digest := sha256.Sum256(file)
	if m.ExecutableSHA256 != hex.EncodeToString(digest[:]) {
		t.Fatal("module hash differs from original executable")
	}
	header := make([]byte, 64)
	if n, err := p.ReadModule(ctx, m, m.Base, header); err != nil || n != len(header) || string(header[:2]) != "MZ" {
		t.Fatalf("module read: n=%d err=%v", n, err)
	}
	if _, err := p.Read(ctx, m.Base, header); err == nil {
		t.Fatal("ordinary private-only Read accepted MEM_IMAGE")
	}
	for _, tc := range []struct {
		name    string
		address uint64
		size    int
	}{
		{"before", m.Base - 1, 1}, {"end", m.Base + m.Size, 1}, {"cross_end", m.Base + m.Size - 1, 2},
		{"overflow", ^uint64(0), 2}, {"private", child.Address, 8}, {"cap", m.Base, (1 << 20) + 1},
	} {
		t.Run(tc.name, func(t *testing.T) {
			if _, err := p.ReadModule(ctx, m, tc.address, make([]byte, tc.size)); err == nil {
				t.Fatal("unbounded/non-module read accepted")
			}
		})
	}
	for _, mutate := range []func(*Module){func(m *Module) { m.Base++ }, func(m *Module) { m.Size++ }, func(m *Module) { m.Name += "x" }, func(m *Module) { m.ExecutableSHA256 = strings.Repeat("0", 64) }} {
		changed := m
		mutate(&changed)
		if changed.LuaImageLayout().validFor(m.ExecutableSHA256, m.Size) {
			t.Fatal("changed module exposed issued layout")
		}
		if _, err := p.ReadModule(ctx, changed, m.Base, header); err == nil {
			t.Fatal("caller-forged module identity accepted")
		}
	}
	if (Module{Name: m.Name, Base: m.Base, Size: m.Size, ExecutableSHA256: m.ExecutableSHA256}).LuaImageLayout().validFor(m.ExecutableSHA256, m.Size) {
		t.Fatal("caller fabricated opaque image layout")
	}
	if _, err := p.ReadModule(ctx, Module{Name: m.Name, Base: m.Base, Size: m.Size, ExecutableSHA256: m.ExecutableSHA256}, m.Base, header); err == nil {
		t.Fatal("unissued module accepted")
	}
	missingLayout := m
	missingProof := *m.proof
	missingProof.layout = LuaRootImageLayout{}
	missingLayout.proof = &missingProof
	if missingLayout.LuaImageLayout().validFor(m.ExecutableSHA256, m.Size) {
		t.Fatal("module without sealed layout exposed a layout")
	}
	if _, err := p.ReadModule(ctx, missingLayout, m.Base, header); err == nil {
		t.Fatal("module without sealed layout accepted")
	}
	second, err := Open(child.PID, child.Created, child.Image)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := second.ReadModule(ctx, m, m.Base, header); err == nil {
		t.Fatal("module from a different Process handle accepted")
	}
	second.Close()
	canceled, stop := context.WithCancel(ctx)
	stop()
	if _, err := p.MainModule(canceled); !errors.Is(err, context.Canceled) {
		t.Fatal("inventory ignored cancellation", err)
	}
	if _, err := p.ReadModule(canceled, m, m.Base, header); !errors.Is(err, context.Canceled) {
		t.Fatal("module read ignored cancellation", err)
	}
	p.Created++
	if _, err := p.ReadModule(ctx, m, m.Base, header); err == nil {
		t.Fatal("changed process generation accepted")
	}
	p.Created--
	if _, err := Open(child.PID, child.Created+1, child.Image); err == nil {
		t.Fatal("restart generation accepted")
	}
	// A query-only process handle cannot silently gain VM_READ permission.
	handle, err := windows.OpenProcess(windows.PROCESS_QUERY_LIMITED_INFORMATION, false, child.PID)
	if err != nil {
		t.Fatal(err)
	}
	restricted := &Process{handle: handle, Created: child.Created, Image: child.Image, PID: child.PID}
	defer restricted.Close()
	rm, err := restricted.MainModule(ctx)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := restricted.ReadModule(ctx, rm, rm.Base, header); err == nil {
		t.Fatal("query-only handle gained module read permission")
	}
	input.Close()
	if err := command.Wait(); err != nil {
		t.Fatal(err)
	}
	waited = true
	if _, err := p.ReadModule(ctx, m, m.Base, header); err == nil {
		t.Fatal("exited process retained module authority")
	}
}

func TestModuleExecutableHashBudgetAndCancellation(t *testing.T) {
	path := t.TempDir() + "/fixture.exe"
	content, err := os.ReadFile(os.Args[0])
	if err != nil {
		t.Fatal(err)
	}
	peFile, err := pe.NewFile(bytes.NewReader(content))
	if err != nil {
		t.Fatal(err)
	}
	moduleSize := uint64(peFile.OptionalHeader.(*pe.OptionalHeader64).SizeOfImage)
	peFile.Close()
	if err := os.WriteFile(path, content, 0600); err != nil {
		t.Fatal(err)
	}
	digest := sha256.Sum256(content)
	got, layout, err := hashModuleExecutable(context.Background(), path, moduleSize)
	if err != nil || got != hex.EncodeToString(digest[:]) || !layout.validFor(got, moduleSize) {
		t.Fatal("incorrect locked file hash/layout", err)
	}
	if layout.validFor(strings.Repeat("0", 64), moduleSize) || layout.validFor(got, moduleSize+4096) {
		t.Fatal("layout lost executable identity")
	}
	canceled, cancel := context.WithCancel(context.Background())
	cancel()
	if _, _, err := hashModuleExecutable(canceled, path, moduleSize); !errors.Is(err, context.Canceled) {
		t.Fatal("hash ignored cancellation", err)
	}
	if err := os.Truncate(path, moduleExecutableLimit+1); err != nil {
		t.Fatal(err)
	}
	if _, _, err := hashModuleExecutable(context.Background(), path, moduleSize); err == nil || err.Error() != "memory.module_executable_limit" {
		t.Fatal("oversized executable hashed", err)
	}
	if err := os.WriteFile(path, []byte("non PE cannot issue a layout"), 0600); err != nil {
		t.Fatal(err)
	}
	if _, _, err := hashModuleExecutable(context.Background(), path, moduleSize); err == nil {
		t.Fatal("non PE hash granted executable layout")
	}
}

// This fixture alone changes its own image-header protection. The adapter under
// test remains read-only and must reject guard/no-access pages before RPM.
func TestNativeModuleProtectionChild(t *testing.T) {
	if os.Getenv("LYCHEE_MODULE_CHILD") != "1" {
		t.Skip("subprocess entry")
	}
	var c, x, k, u windows.Filetime
	if err := windows.GetProcessTimes(windows.CurrentProcess(), &c, &x, &k, &u); err != nil {
		t.Fatal(err)
	}
	image, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	created := uint64(c.HighDateTime)<<32 | uint64(c.LowDateTime)
	p, err := Open(uint32(os.Getpid()), created, image)
	if err != nil {
		t.Fatal(err)
	}
	defer p.Close()
	m, err := p.MainModule(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	encoder := json.NewEncoder(os.Stdout)
	if err := encoder.Encode(childMemory{PID: p.PID, Created: created, Image: image, Address: m.Base}); err != nil {
		t.Fatal(err)
	}
	scanner := bufio.NewScanner(os.Stdin)
	var original uint32
	changed := false
	defer func() {
		if changed {
			var old uint32
			_ = windows.VirtualProtect(uintptr(m.Base), 4096, original, &old)
		}
	}()
	for scanner.Scan() {
		protect := uint32(windows.PAGE_NOACCESS)
		if scanner.Text() == "guard" {
			protect = windows.PAGE_READONLY | windows.PAGE_GUARD
		}
		var old uint32
		if err := windows.VirtualProtect(uintptr(m.Base), 4096, protect, &old); err != nil {
			t.Fatal(err)
		}
		if !changed {
			original = old
			changed = true
		}
		if err := encoder.Encode("ready"); err != nil {
			t.Fatal(err)
		}
	}
}

func TestMainModuleRejectsProtectedImagePages(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()
	command := exec.CommandContext(ctx, os.Args[0], "-test.run=^TestNativeModuleProtectionChild$")
	command.Env = append(os.Environ(), "LYCHEE_MODULE_CHILD=1")
	command.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}
	input, err := command.StdinPipe()
	if err != nil {
		t.Fatal(err)
	}
	output, err := command.StdoutPipe()
	if err != nil {
		t.Fatal(err)
	}
	var diagnostics bytes.Buffer
	command.Stderr = &diagnostics
	if err := command.Start(); err != nil {
		t.Fatal(err)
	}
	defer func() {
		input.Close()
		if err := command.Wait(); err != nil {
			t.Errorf("child: %v %s", err, diagnostics.String())
		}
	}()
	decoder := json.NewDecoder(output)
	var child childMemory
	if err := decoder.Decode(&child); err != nil {
		t.Fatal(err)
	}
	p, err := Open(child.PID, child.Created, child.Image)
	if err != nil {
		t.Fatal(err)
	}
	defer p.Close()
	m, err := p.MainModule(ctx)
	if err != nil {
		t.Fatal(err)
	}
	for _, mode := range []string{"guard", "noaccess"} {
		if _, err := io.WriteString(input, mode+"\n"); err != nil {
			t.Fatal(err)
		}
		var ack string
		if err := decoder.Decode(&ack); err != nil || ack != "ready" {
			t.Fatal("protection barrier failed", err)
		}
		if _, err := p.ReadModule(ctx, m, m.Base, make([]byte, 8)); err == nil || !strings.Contains(err.Error(), "memory.ineligible_module_read reason=current_protection") || !strings.Contains(err.Error(), "stage=before_read") {
			t.Fatal("protected image page accepted", mode, err)
		}
	}
}

func TestModuleMappingEligibility(t *testing.T) {
	base := uintptr(0x10000)
	good := windows.MemoryBasicInformation{BaseAddress: base, AllocationBase: base, RegionSize: 4096, State: windows.MEM_COMMIT, Protect: windows.PAGE_READONLY, Type: 0x1000000}
	for _, tc := range []struct {
		name   string
		change func(*windows.MemoryBasicInformation)
	}{
		{"guard", func(i *windows.MemoryBasicInformation) { i.Protect |= windows.PAGE_GUARD }},
		{"noaccess", func(i *windows.MemoryBasicInformation) { i.Protect = windows.PAGE_NOACCESS }},
		{"execute_only", func(i *windows.MemoryBasicInformation) { i.Protect = windows.PAGE_EXECUTE }},
		{"reserved", func(i *windows.MemoryBasicInformation) { i.State = windows.MEM_RESERVE }},
		{"private", func(i *windows.MemoryBasicInformation) { i.Type = 0x20000 }},
		{"different_image", func(i *windows.MemoryBasicInformation) { i.AllocationBase += 4096 }},
		{"empty", func(i *windows.MemoryBasicInformation) { i.RegionSize = 0 }},
		{"overflow", func(i *windows.MemoryBasicInformation) { i.RegionSize = ^uintptr(0) }},
	} {
		t.Run(tc.name, func(t *testing.T) {
			changed := good
			tc.change(&changed)
			if _, ok := moduleMappingEnd(changed, uint64(base), uint64(base), uint64(base)+8192); ok {
				t.Fatal("ineligible image mapping accepted")
			}
		})
	}
	if end, ok := moduleMappingEnd(good, uint64(base), uint64(base), uint64(base)+8192); !ok || end != uint64(base)+4096 {
		t.Fatal("readable image mapping rejected")
	}
}

// Captured Retail module facts: its code/header and second data allocation are
// MEM_MAPPED, although both lie inside the OS main-module inventory boundary.
func TestModuleCapturedMappedSegments(t *testing.T) {
	base := uint64(0x7ff79a670000)
	for _, info := range []windows.MemoryBasicInformation{
		{BaseAddress: uintptr(base), AllocationBase: uintptr(base), RegionSize: 90767360, State: windows.MEM_COMMIT, Protect: windows.PAGE_EXECUTE_READ, Type: 0x40000},
		{BaseAddress: 0x7ff7a2030000, AllocationBase: 0x7ff79fd00000, RegionSize: 5636096, State: windows.MEM_COMMIT, Protect: windows.PAGE_READWRITE, Type: 0x40000},
	} {
		if _, ok := moduleMappingEnd(info, uint64(info.BaseAddress), base, base+136511488); !ok {
			t.Fatal("OS-owned readable module segment rejected", info.Type)
		}
	}
}

func moduleSegmentFixture(at uint64) (windows.MemoryBasicInformation, error) {
	for _, info := range []windows.MemoryBasicInformation{
		{BaseAddress: 0x10000, AllocationBase: 0x10000, RegionSize: 0x2000, State: windows.MEM_COMMIT, Protect: windows.PAGE_EXECUTE_READ, Type: moduleMappedType},
		{BaseAddress: 0x12000, AllocationBase: 0x12000, RegionSize: 0x2000, State: windows.MEM_COMMIT, Protect: windows.PAGE_READWRITE, Type: moduleMappedType},
		{BaseAddress: 0x14000, RegionSize: 0x1000, State: windows.MEM_RESERVE},
		{BaseAddress: 0x15000, AllocationBase: 0x15000, RegionSize: 0x3000, State: windows.MEM_COMMIT, Protect: windows.PAGE_READONLY, Type: moduleImageType},
	} {
		if uint64(info.BaseAddress) <= at && at < uint64(info.BaseAddress+info.RegionSize) {
			return info, nil
		}
	}
	return windows.MemoryBasicInformation{}, io.EOF
}
func TestModuleSealedSegmentPlan(t *testing.T) {
	ctx := context.Background()
	plan, err := captureModuleSegments(ctx, 0x10000, 0x6000, moduleSegmentFixture)
	if err != nil || len(plan) != 4 || plan[2].eligible || plan[3].end != 0x16000 || plan[3].regionEnd != 0x18000 {
		t.Fatal("incorrect clamped/ineligible plan", plan, err)
	}
	if err := verifyModuleSegments(ctx, 0x10000, 0x6000, 0x10020, 0x13020, plan, moduleSegmentFixture); err != nil {
		t.Fatal("mapped allocation transition rejected", err)
	}
	if err := verifyModuleSegments(ctx, 0x10000, 0x6000, 0x15000, 0x16000, plan, moduleSegmentFixture); err != nil {
		t.Fatal("clamped image tail rejected", err)
	}
	for name, change := range map[string]func(*windows.MemoryBasicInformation){
		"type":               func(i *windows.MemoryBasicInformation) { i.Type = moduleImageType },
		"allocation":         func(i *windows.MemoryBasicInformation) { i.AllocationBase = 0x11000 },
		"outside_allocation": func(i *windows.MemoryBasicInformation) { i.AllocationBase = 0xf000 },
		"start":              func(i *windows.MemoryBasicInformation) { i.BaseAddress = 0x11000; i.RegionSize = 0x3000 },
		"end_inside_request": func(i *windows.MemoryBasicInformation) { i.RegionSize = 0x20 },
		"guard":              func(i *windows.MemoryBasicInformation) { i.Protect |= windows.PAGE_GUARD },
		"noaccess":           func(i *windows.MemoryBasicInformation) { i.Protect = windows.PAGE_NOACCESS },
		"reserve":            func(i *windows.MemoryBasicInformation) { i.State = windows.MEM_RESERVE },
		"private":            func(i *windows.MemoryBasicInformation) { i.Type = 0x20000 },
	} {
		t.Run(name, func(t *testing.T) {
			query := func(at uint64) (windows.MemoryBasicInformation, error) {
				info, err := moduleSegmentFixture(at)
				if at >= 0x12000 && at < 0x14000 {
					change(&info)
				}
				return info, err
			}
			if err := verifyModuleSegments(ctx, 0x10000, 0x6000, 0x12020, 0x12028, plan, query); err == nil {
				t.Fatal("changed sealed mapping accepted")
			}
		})
	}
	missing := append([]moduleSegment(nil), plan[:1]...)
	missing = append(missing, plan[2:]...)
	if err := verifyModuleSegments(ctx, 0x10000, 0x6000, 0x12000, 0x12008, missing, moduleSegmentFixture); err == nil {
		t.Fatal("missing segment admitted")
	}
	queries := 0
	if err := verifyModuleSegments(ctx, 0x10000, 0x6000, 0x14000, 0x14008, plan, func(at uint64) (windows.MemoryBasicInformation, error) { queries++; return moduleSegmentFixture(at) }); err == nil || queries != 0 {
		t.Fatal("ineligible issuance revived")
	}
	if err := verifyModuleSegments(ctx, 0x10000, 0x6000, 0x15ff0, 0x16001, plan, moduleSegmentFixture); err == nil {
		t.Fatal("escaped OS inventory end")
	}
	canceled, cancel := context.WithCancel(ctx)
	query := func(at uint64) (windows.MemoryBasicInformation, error) { cancel(); return moduleSegmentFixture(at) }
	if err := verifyModuleSegments(canceled, 0x10000, 0x6000, 0x10000, 0x10008, plan, query); !errors.Is(err, context.Canceled) {
		t.Fatal("query cancellation lost", err)
	}
}

func TestModuleMappingDiagnostics(t *testing.T) {
	plan, err := captureModuleSegments(context.Background(), 0x10000, 0x6000, moduleSegmentFixture)
	if err != nil {
		t.Fatal(err)
	}
	for _, tc := range []struct {
		name, reason string
		address      uint64
		change       func(*windows.MemoryBasicInformation)
		queries      int
	}{
		{"ineligible_issuance", "issued_ineligible", 0x14000, nil, 0},
		{"changed_type", "changed_type", 0x12020, func(i *windows.MemoryBasicInformation) { i.Type = moduleImageType }, 1},
		{"changed_allocation", "changed_allocation", 0x12020, func(i *windows.MemoryBasicInformation) { i.AllocationBase = 0x11000 }, 1},
		{"merged_other_allocation", "changed_allocation", 0x12020, func(i *windows.MemoryBasicInformation) {
			i.BaseAddress = 0x11000
			i.AllocationBase = 0x11000
			i.RegionSize = 0x3000
		}, 1},
		{"changed_end", "current_geometry", 0x13020, func(i *windows.MemoryBasicInformation) { i.RegionSize = 0x1000 }, 1},
		{"current_protection", "current_protection", 0x12020, func(i *windows.MemoryBasicInformation) { i.Protect |= windows.PAGE_GUARD }, 1},
		{"current_state", "current_state", 0x12020, func(i *windows.MemoryBasicInformation) { i.State = windows.MEM_RESERVE }, 1},
	} {
		t.Run(tc.name, func(t *testing.T) {
			queries := 0
			query := func(at uint64) (windows.MemoryBasicInformation, error) {
				queries++
				info, err := moduleSegmentFixture(at)
				if tc.change != nil {
					tc.change(&info)
				}
				return info, err
			}
			err := verifyModuleSegments(context.Background(), 0x10000, 0x6000, tc.address, tc.address+8, plan, query)
			if err == nil || queries != tc.queries {
				t.Fatal("missing rejection or additional diagnostic reads", err, queries)
			}
			for _, field := range []string{"memory.ineligible_module_read", "reason=" + tc.reason, "address=0x", "module=[", "issued={", "current={", "state=", "protect="} {
				if !strings.Contains(err.Error(), field) {
					t.Fatalf("missing %q in %v", field, err)
				}
			}
			if len(err.Error()) > 1024 {
				t.Fatal("unbounded module diagnostic")
			}
		})
	}
}

func TestModuleRequestedSpanIntersection(t *testing.T) {
	plan, err := captureModuleSegments(context.Background(), 0x10000, 0x6000, moduleSegmentFixture)
	if err != nil {
		t.Fatal(err)
	}
	for _, tc := range []struct {
		name    string
		end     uint64
		size    uintptr
		protect uint32
		want    bool
		queries int
	}{
		{"remote_shrink", 0x12028, 0x1000, windows.PAGE_READWRITE, true, 2},
		{"remote_expand", 0x12028, 0x4000, windows.PAGE_READWRITE, true, 2},
		{"shrink_inside_request", 0x13020, 0x1000, windows.PAGE_READWRITE, false, 4},
		{"expand_cannot_fill_old_hole", 0x14008, 0x4000, windows.PAGE_READWRITE, false, 2},
		{"readable_protection_changed", 0x12028, 0x2000, windows.PAGE_READONLY, false, 2},
	} {
		t.Run(tc.name, func(t *testing.T) {
			queries := 0
			query := func(at uint64) (windows.MemoryBasicInformation, error) {
				queries++
				info, err := moduleSegmentFixture(at)
				if at >= 0x12000 && at < 0x14000 {
					info.RegionSize = tc.size
					info.Protect = tc.protect
				}
				return info, err
			}
			// ReadModule invokes this same verifier before and after physical IO.
			for _, stage := range []string{"before", "after"} {
				err := verifyModuleSegments(context.Background(), 0x10000, 0x6000, 0x12020, tc.end, plan, query)
				if (err == nil) != tc.want {
					t.Fatalf("%s requested-span gate: %v", stage, err)
				}
			}
			if queries != tc.queries {
				t.Fatalf("intersection queried outside issued eligible span: %d", queries)
			}
		})
	}
}

func TestModuleMultipleCurrentPiecesRetainIssuedAuthorization(t *testing.T) {
	plan, err := captureModuleSegments(context.Background(), 0x10000, 0x6000, moduleSegmentFixture)
	if err != nil {
		t.Fatal(err)
	}
	for _, mode := range []string{"split", "protect", "type", "allocation", "state", "unaligned_start", "unaligned_size", "cancel"} {
		t.Run(mode, func(t *testing.T) {
			ctx, cancel := context.WithCancel(context.Background())
			defer cancel()
			var locations []uint64
			query := func(at uint64) (windows.MemoryBasicInformation, error) {
				locations = append(locations, at)
				info, err := moduleSegmentFixture(at)
				info.BaseAddress = uintptr(at &^ 4095)
				info.RegionSize = 4096
				if at == 0x11000 {
					switch mode {
					case "protect":
						info.Protect = windows.PAGE_READONLY
					case "type":
						info.Type = moduleImageType
					case "allocation":
						info.AllocationBase = 0x11000
					case "state":
						info.State = windows.MEM_RESERVE
					case "unaligned_start":
						info.BaseAddress++
					case "unaligned_size":
						info.RegionSize--
					case "cancel":
						cancel()
					}
				}
				return info, err
			}
			err := verifyModuleSegments(ctx, 0x10000, 0x6000, 0x10020, 0x13020, plan, query)
			if mode == "split" {
				if err != nil || len(locations) != 4 || locations[0] != 0x10020 || locations[1] != 0x11000 || locations[2] != 0x12000 || locations[3] != 0x13000 {
					t.Fatal("split current/issued traversal", err, locations)
				}
			} else if err == nil {
				t.Fatal("changed requested current piece authorized", mode)
			}
			if mode == "cancel" && !errors.Is(err, context.Canceled) {
				t.Fatal("piece cancellation lost", err)
			}
			if len(locations) > 4 {
				t.Fatal("query budget exceeded requested pages", locations)
			}
		})
	}
}
func TestModuleSegmentCaptureBoundariesAndBudget(t *testing.T) {
	for name, info := range map[string]windows.MemoryBasicInformation{
		"empty":           {BaseAddress: 0x10000},
		"overflow":        {BaseAddress: 0x10000, RegionSize: ^uintptr(0)},
		"gap":             {BaseAddress: 0x11000, RegionSize: 0x1000},
		"nonprogress":     {BaseAddress: 0xf000, RegionSize: 0x1000},
		"unaligned_start": {BaseAddress: 0xffff, RegionSize: 0x2000},
		"unaligned_size":  {BaseAddress: 0x10000, RegionSize: 1},
	} {
		t.Run(name, func(t *testing.T) {
			if _, err := captureModuleSegments(context.Background(), 0x10000, 0x2000, func(uint64) (windows.MemoryBasicInformation, error) { return info, nil }); err == nil {
				t.Fatal("invalid region geometry issued")
			}
		})
	}
	count := 0
	plan, err := captureModuleSegments(context.Background(), 0x10000, uint64(moduleInventoryLimit+1)*4096, func(at uint64) (windows.MemoryBasicInformation, error) {
		count++
		return windows.MemoryBasicInformation{BaseAddress: uintptr(at), RegionSize: 4096, State: windows.MEM_RESERVE}, nil
	})
	if err != nil || count != moduleInventoryLimit+1 || len(plan) != count {
		t.Fatal("loader inventory budget incorrectly limits module mappings", count, err)
	}
	for name, bounds := range map[string][2]uint64{
		"oversized":      {0x10000, moduleVirtualLimit + 4096},
		"overflow":       {^uint64(0) - 4095, 4096},
		"unaligned_base": {0x10001, 4096},
		"unaligned_size": {0x10000, 4097},
		"empty":          {0x10000, 0},
	} {
		t.Run(name, func(t *testing.T) {
			queries := 0
			_, err := captureModuleSegments(context.Background(), bounds[0], bounds[1], func(at uint64) (windows.MemoryBasicInformation, error) { queries++; return moduleSegmentFixture(at) })
			if err == nil || queries != 0 {
				t.Fatal("invalid module range queried", err, queries)
			}
		})
	}
	for _, tc := range []struct {
		base, size, page, want uint64
		valid                  bool
	}{
		{0x10000, 4096, 4096, 1, true},
		{0x10000, moduleVirtualLimit, 4096, moduleVirtualLimit / 4096, true},
		{0x10000, 8192, 8192, 1, true},
		{0x10000, 4096, 0, 0, false},
		{0x10000, 4096, 8192, 0, false},
	} {
		capacity, valid := moduleMappingCapacity(tc.base, tc.size, tc.page)
		if capacity != tc.want || valid != tc.valid {
			t.Fatal("mapping capacity does not follow system page geometry", tc, capacity, valid)
		}
	}
	canceled, cancel := context.WithCancel(context.Background())
	_, err = captureModuleSegments(canceled, 0x10000, 0x2000, func(at uint64) (windows.MemoryBasicInformation, error) { cancel(); return moduleSegmentFixture(at) })
	if !errors.Is(err, context.Canceled) {
		t.Fatal("last capture cancellation lost", err)
	}
	_, err = captureModuleSegments(context.Background(), 0x10000, 0x2000, func(uint64) (windows.MemoryBasicInformation, error) {
		return windows.MemoryBasicInformation{}, io.ErrUnexpectedEOF
	})
	if !errors.Is(err, io.ErrUnexpectedEOF) {
		t.Fatal("query failure discarded", err)
	}
}

// This child creates three ordinary anonymous mapped pages outside its
// executable. Production is read-only; Module must not authorize these pages.
func TestNativeModuleMappedChild(t *testing.T) {
	if os.Getenv("LYCHEE_MODULE_MAPPED_CHILD") != "1" {
		t.Skip("subprocess entry")
	}
	mapping, err := windows.CreateFileMapping(windows.InvalidHandle, nil, windows.PAGE_READWRITE, 0, 3*4096, nil)
	if err != nil {
		t.Fatal(err)
	}
	defer windows.CloseHandle(mapping)
	address, err := windows.MapViewOfFile(mapping, windows.FILE_MAP_READ|windows.FILE_MAP_WRITE, 0, 0, 3*4096)
	if err != nil {
		t.Fatal(err)
	}
	defer windows.UnmapViewOfFile(address)
	var c, x, k, u windows.Filetime
	if err := windows.GetProcessTimes(windows.CurrentProcess(), &c, &x, &k, &u); err != nil {
		t.Fatal(err)
	}
	image, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	child := childMemory{PID: uint32(os.Getpid()), Created: uint64(c.HighDateTime)<<32 | uint64(c.LowDateTime), Image: image, Address: uint64(address)}
	if err := json.NewEncoder(os.Stdout).Encode(child); err != nil {
		t.Fatal(err)
	}
	scanner := bufio.NewScanner(os.Stdin)
	for scanner.Scan() {
		protect := uint32(windows.PAGE_READWRITE)
		page := uintptr(2 * 4096)
		if scanner.Text() == "front" {
			page = 0
			protect = windows.PAGE_NOACCESS
		}
		if scanner.Text() == "middle" {
			page = 4096
			protect = windows.PAGE_READONLY
		}
		if scanner.Text() == "tail" {
			protect = windows.PAGE_NOACCESS
		}
		var old uint32
		if scanner.Text() == "restore" {
			page = 0
		}
		length := uintptr(4096)
		if scanner.Text() == "restore" {
			length = 3 * 4096
		}
		if err := windows.VirtualProtect(address+page, length, protect, &old); err != nil {
			t.Fatal(err)
		}
		if err := json.NewEncoder(os.Stdout).Encode("ready"); err != nil {
			t.Fatal(err)
		}
	}
}

func TestModuleMappedInteriorQuery(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()
	command := exec.CommandContext(ctx, os.Args[0], "-test.run=^TestNativeModuleMappedChild$")
	command.Env = append(os.Environ(), "LYCHEE_MODULE_MAPPED_CHILD=1")
	command.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}
	input, err := command.StdinPipe()
	if err != nil {
		t.Fatal(err)
	}
	output, err := command.StdoutPipe()
	if err != nil {
		t.Fatal(err)
	}
	var diagnostics bytes.Buffer
	command.Stderr = &diagnostics
	if err := command.Start(); err != nil {
		t.Fatal(err)
	}
	defer func() {
		input.Close()
		if err := command.Wait(); err != nil {
			t.Errorf("child: %v %s", err, diagnostics.String())
		}
	}()
	var child childMemory
	decoder := json.NewDecoder(output)
	if err := decoder.Decode(&child); err != nil {
		t.Fatal(err)
	}
	p, err := Open(child.PID, child.Created, child.Image)
	if err != nil {
		t.Fatal(err)
	}
	defer p.Close()
	baseInfo, err := p.queryModuleMapping(child.Address)
	if err != nil || baseInfo.Type != moduleMappedType || uint64(baseInfo.RegionSize) < 3*4096 {
		t.Fatal("fixture is not three contiguous mapped pages", err, baseInfo)
	}
	middle := child.Address + 4096
	middleInfo, err := p.queryModuleMapping(middle)
	if err != nil || uint64(middleInfo.BaseAddress) != middle || uint64(middleInfo.BaseAddress)+uint64(middleInfo.RegionSize) != uint64(baseInfo.BaseAddress)+uint64(baseInfo.RegionSize) {
		t.Fatal("fixture did not reproduce forward-only query geometry", err, baseInfo, middleInfo)
	}
	// Test the segment verifier with real OS mappings. This helper plan never
	// issues a public Module or grants ReadModule access to non-executable pages.
	plan, err := captureModuleSegments(ctx, child.Address, 3*4096, p.queryModuleMapping)
	if err != nil {
		t.Fatal(err)
	}
	queries := 0
	query := func(at uint64) (windows.MemoryBasicInformation, error) {
		queries++
		return p.queryModuleMapping(at)
	}
	if err := verifyModuleSegments(ctx, child.Address, 3*4096, middle, middle+4096, plan, query); err != nil {
		t.Fatal("unchanged mapped interior rejected", err)
	}
	if queries != 1 {
		t.Fatal("interior verification added queries", queries)
	}
	if _, err := io.WriteString(input, "front\n"); err != nil {
		t.Fatal(err)
	}
	var frontAck string
	if err := decoder.Decode(&frontAck); err != nil || frontAck != "ready" {
		t.Fatal("front barrier", err)
	}
	if err := verifyModuleSegments(ctx, child.Address, 3*4096, middle, middle+49, plan, p.queryModuleMapping); err != nil {
		t.Fatal("before-read front split", err)
	}
	frontBytes := make([]byte, 49)
	var frontN uintptr
	if err := windows.ReadProcessMemory(p.handle, uintptr(middle), &frontBytes[0], 49, &frontN); err != nil || frontN != 49 {
		t.Fatal("front-split helper read", err, frontN)
	}
	if err := verifyModuleSegments(ctx, child.Address, 3*4096, middle, middle+49, plan, p.queryModuleMapping); err != nil {
		t.Fatal("after-read front split", err)
	}
	if err := verifyModuleSegments(ctx, child.Address, 3*4096, child.Address, child.Address+49, plan, p.queryModuleMapping); err == nil {
		t.Fatal("unreadable front page authorized")
	}
	if _, err := io.WriteString(input, "middle\n"); err != nil {
		t.Fatal(err)
	}
	if err := decoder.Decode(&frontAck); err != nil || frontAck != "ready" {
		t.Fatal("middle barrier", err)
	}
	if err := verifyModuleSegments(ctx, child.Address, 3*4096, middle, middle+49, plan, p.queryModuleMapping); err == nil {
		t.Fatal("changed middle protection authorized")
	}
	if _, err := io.WriteString(input, "restore\n"); err != nil {
		t.Fatal(err)
	}
	if err := decoder.Decode(&frontAck); err != nil || frontAck != "ready" {
		t.Fatal("front restore barrier", err)
	}
	// A far page's protection splits the original three-page OS run. The
	// requested middle bytes remain authorized by both current and issued spans.
	if _, err := io.WriteString(input, "tail\n"); err != nil {
		t.Fatal(err)
	}
	var ack string
	if err := decoder.Decode(&ack); err != nil || ack != "ready" {
		t.Fatal("tail barrier", err)
	}
	if err := verifyModuleSegments(ctx, child.Address, 3*4096, middle, middle+49, plan, p.queryModuleMapping); err != nil {
		t.Fatal("before-read remote split", err)
	}
	b := make([]byte, 49)
	var n uintptr
	if err := windows.ReadProcessMemory(p.handle, uintptr(middle), &b[0], uintptr(len(b)), &n); err != nil || n != 49 {
		t.Fatal("helper middle read", err, n)
	}
	if err := verifyModuleSegments(ctx, child.Address, 3*4096, middle, middle+49, plan, p.queryModuleMapping); err != nil {
		t.Fatal("after-read remote split", err)
	}
	if err := verifyModuleSegments(ctx, child.Address, 3*4096, middle, middle+4096+8, plan, p.queryModuleMapping); err == nil {
		t.Fatal("shrink cutting requested bytes accepted")
	}
	holePlan, err := captureModuleSegments(ctx, child.Address, 3*4096, p.queryModuleMapping)
	if err != nil || len(holePlan) != 2 || holePlan[1].eligible {
		t.Fatal("helper hole plan", err)
	}
	if _, err := io.WriteString(input, "restore\n"); err != nil {
		t.Fatal(err)
	}
	if err := decoder.Decode(&ack); err != nil || ack != "ready" {
		t.Fatal("restore barrier", err)
	}
	if err := verifyModuleSegments(ctx, child.Address, 3*4096, middle, middle+49, holePlan, p.queryModuleMapping); err != nil {
		t.Fatal("remote expanded run rejected", err)
	}
	if err := verifyModuleSegments(ctx, child.Address, 3*4096, middle, middle+4096+8, holePlan, p.queryModuleMapping); err == nil {
		t.Fatal("newly readable extension revived old hole")
	}
}

func TestMainModuleRejectsExternalMappedChild(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()
	command := exec.CommandContext(ctx, os.Args[0], "-test.run=^TestNativeModuleMappedChild$")
	command.Env = append(os.Environ(), "LYCHEE_MODULE_MAPPED_CHILD=1")
	command.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}
	input, err := command.StdinPipe()
	if err != nil {
		t.Fatal(err)
	}
	output, err := command.StdoutPipe()
	if err != nil {
		t.Fatal(err)
	}
	var diagnostics bytes.Buffer
	command.Stderr = &diagnostics
	if err := command.Start(); err != nil {
		t.Fatal(err)
	}
	defer func() {
		input.Close()
		if err := command.Wait(); err != nil {
			t.Errorf("child: %v %s", err, diagnostics.String())
		}
	}()
	var child childMemory
	if err := json.NewDecoder(output).Decode(&child); err != nil {
		t.Fatal(err)
	}
	p, err := Open(child.PID, child.Created, child.Image)
	if err != nil {
		t.Fatal(err)
	}
	defer p.Close()
	info, err := p.queryModuleMapping(child.Address)
	if err != nil || info.Type != moduleMappedType || !readable(info.Protect) {
		t.Fatal("fixture is not readable mapped memory", err)
	}
	m, err := p.MainModule(ctx)
	if err != nil {
		t.Fatal(err)
	}
	if child.Address >= m.Base && child.Address < m.Base+m.Size {
		t.Fatal("fixture unexpectedly inside executable")
	}
	if _, err := p.ReadModule(ctx, m, child.Address, make([]byte, 8)); err == nil {
		t.Fatal("module seal admitted unrelated mapped page")
	}
	forged := Module{Name: m.Name, Base: child.Address, Size: 4096, ExecutableSHA256: m.ExecutableSHA256}
	if _, err := p.ReadModule(ctx, forged, child.Address, make([]byte, 8)); err == nil {
		t.Fatal("caller invented mapped module")
	}
	changed := m
	changed.Base = child.Address
	changed.Size = 4096
	if _, err := p.ReadModule(ctx, changed, child.Address, make([]byte, 8)); err == nil {
		t.Fatal("sealed image moved to mapped page")
	}
	if _, err := p.Read(ctx, child.Address, make([]byte, 8)); err == nil {
		t.Fatal("ordinary Read accepted mapped page")
	}
}

// Forever 1.60.1.70124 has 4,955 committed MEM_MAPPED regions in its
// 140,742,656-byte OS main module, all page-aligned and at its base allocation.
// Reproduce that inventory envelope with maximally fragmented leading pages,
// including the captured alternating readable/noaccess code protections.
func TestModuleFragmentedForeverInventory(t *testing.T) {
	const base, size, count = uint64(0x7ff7f71e0000), uint64(140742656), 4955
	query := func(at uint64) (windows.MemoryBasicInformation, error) {
		at &= ^uint64(4095) // VirtualQuery rounds a point query down to its page.
		index := (at - base) / 4096
		length := uint64(4096)
		if index == count-1 {
			length = size - index*4096
		}
		protect := uint32(windows.PAGE_EXECUTE_READ)
		if index%2 != 0 {
			protect = windows.PAGE_NOACCESS
		}
		return windows.MemoryBasicInformation{BaseAddress: uintptr(at), AllocationBase: uintptr(base), RegionSize: uintptr(length), State: windows.MEM_COMMIT, Protect: protect, Type: moduleMappedType}, nil
	}
	plan, err := captureModuleSegments(context.Background(), base, size, query)
	if err != nil || len(plan) != count {
		t.Fatalf("valid fragmented OS inventory rejected: count=%d err=%v", len(plan), err)
	}
	// Deep read requires exactly one actual-location query, despite 4,955 spans.
	queries := 0
	at := base + uint64(count-3)*4096 + 24
	err = verifyModuleSegments(context.Background(), base, size, at, at+8, plan, func(origin uint64) (windows.MemoryBasicInformation, error) { queries++; return query(origin) })
	if err != nil || queries != 1 {
		t.Fatalf("deep fragment: queries=%d err=%v", queries, err)
	}
	// A crossing read still refuses the adjacent sealed noaccess page.
	at = base + uint64(count-3)*4096 + 4090
	if err := verifyModuleSegments(context.Background(), base, size, at, at+12, plan, query); err == nil {
		t.Fatal("fragmented noaccess hole admitted")
	}
}
