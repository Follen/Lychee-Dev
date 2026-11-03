//go:build windows && amd64

package publish

import (
	"bytes"
	"context"
	"golang.org/x/sys/windows"
	"io"
	"os"
	"path/filepath"
	"strings"
	"unsafe"
)

type ddProcessRow struct {
	pid, parent uint32
	name        string
}
type ddDesktop struct {
	guiPID, networkPID                  int
	guiCreated, networkCreated          uint64
	installation, networkImage, profile string
}

func ddProcessRows() (rows []ddProcessRow, err error) {
	handle, err := windows.CreateToolhelp32Snapshot(windows.TH32CS_SNAPPROCESS, 0)
	if err != nil {
		return nil, ddCookieError()
	}
	defer windows.CloseHandle(handle)
	entry := windows.ProcessEntry32{Size: uint32(unsafe.Sizeof(windows.ProcessEntry32{}))}
	if windows.Process32First(handle, &entry) != nil {
		return nil, ddCookieError()
	}
	for {
		rows = append(rows, ddProcessRow{entry.ProcessID, entry.ParentProcessID, windows.UTF16ToString(entry.ExeFile[:])})
		if windows.Process32Next(handle, &entry) != nil {
			break
		}
	}
	return rows, nil
}
func ddProcessImage(pid uint32, args bool) (string, []string, error) {
	handle, err := windows.OpenProcess(windows.PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
	if err != nil {
		return "", nil, ddCookieError()
	}
	defer windows.CloseHandle(handle)
	buf := make([]uint16, 32768)
	size := uint32(len(buf))
	if windows.QueryFullProcessImageName(handle, 0, &buf[0], &size) != nil {
		return "", nil, ddCookieError()
	}
	path := windows.UTF16ToString(buf[:size])
	if !args {
		return path, nil, nil
	}
	var needed uint32
	windows.NtQueryInformationProcess(handle, 60, nil, 0, &needed)
	if needed < 16 || needed > 131072 {
		return "", nil, ddCookieError()
	}
	data := make([]byte, needed)
	if windows.NtQueryInformationProcess(handle, 60, unsafe.Pointer(&data[0]), needed, &needed) != nil {
		return "", nil, ddCookieError()
	}
	type unicodeString struct {
		Length, Capacity uint16
		Buffer           *uint16
	}
	text := (*unicodeString)(unsafe.Pointer(&data[0]))
	start := uintptr(unsafe.Pointer(&data[0]))
	pointer := uintptr(unsafe.Pointer(text.Buffer))
	if text.Length%2 != 0 || pointer < start || pointer > start+uintptr(len(data)) || uintptr(text.Length) > start+uintptr(len(data))-pointer {
		return "", nil, ddCookieError()
	}
	wide := append([]uint16(nil), unsafe.Slice(text.Buffer, int(text.Length)/2)...)
	wide = append(wide, 0)
	var count int32
	argv, err := windows.CommandLineToArgv(&wide[0], &count)
	if err != nil {
		return "", nil, ddCookieError()
	}
	defer windows.LocalFree(windows.Handle(unsafe.Pointer(argv)))
	if count < 1 || count > 1024 {
		return "", nil, ddCookieError()
	}
	out := []string{}
	for _, arg := range unsafe.Slice((**uint16)(unsafe.Pointer(argv)), int(count)) {
		out = append(out, windows.UTF16PtrToString(arg))
	}
	return path, out, nil
}
func ddSelectDesktop(ctx context.Context, installation string) (*ddDesktop, error) {
	rows, err := ddProcessRows()
	if err != nil {
		return nil, err
	}
	guis := []ddProcessRow{}
	for _, row := range rows {
		if strings.EqualFold(row.name, "netease_dd.exe") {
			guis = append(guis, row)
		}
	}
	if len(guis) == 0 {
		return nil, nil
	}
	if len(guis) != 1 {
		return nil, capability("publish.dd_desktop", "multiple DD desktop processes; keep one selected desktop instance")
	}
	gui := guis[0]
	image, _, err := ddProcessImage(gui.pid, false)
	if err != nil {
		return nil, err
	}
	absolute, err := filepath.EvalSymlinks(image)
	if err != nil || !strings.EqualFold(filepath.Dir(absolute), installation) {
		return nil, capability("publish.dd_desktop", "the running DD desktop differs from the selected installation")
	}
	if err = ddVerifiedPublisher(ctx, image, "NetEase (Hangzhou) Network Co., Ltd"); err != nil {
		return nil, err
	}
	created, err := ddProcessCreated(int(gui.pid))
	if err != nil {
		return nil, ddCookieError()
	}
	root, err := ddRoaming()
	if err != nil {
		return nil, ddCookieError()
	}
	root, err = filepath.EvalSymlinks(filepath.Join(root, "CCVoiceHub", "web_cache"))
	if err != nil {
		return nil, ddCookieError()
	}
	desc := map[uint32]bool{gui.pid: true}
	for i := 0; i < 8; i++ {
		changed := false
		for _, row := range rows {
			if desc[row.parent] && !desc[row.pid] {
				desc[row.pid] = true
				changed = true
			}
		}
		if !changed {
			break
		}
	}
	var selected *ddDesktop
	for _, row := range rows {
		if !desc[row.pid] || !strings.EqualFold(row.name, "msedgewebview2.exe") {
			continue
		}
		if err = ctx.Err(); err != nil {
			return nil, err
		}
		image, args, e := ddProcessImage(row.pid, true)
		if e != nil {
			continue
		}
		network := false
		profiles := []string{}
		for _, arg := range args {
			if arg == "--utility-sub-type=network.mojom.NetworkService" {
				network = true
			}
			if value, ok := strings.CutPrefix(arg, "--user-data-dir="); ok {
				profiles = append(profiles, value)
			}
		}
		if !network {
			continue
		}
		if len(profiles) != 1 || !filepath.IsAbs(profiles[0]) {
			return nil, ddCookieError()
		}
		profile, e := filepath.EvalSymlinks(profiles[0])
		if e != nil {
			return nil, ddCookieError()
		}
		rel, e := filepath.Rel(root, profile)
		if e != nil || rel == ".." || strings.HasPrefix(rel, ".."+string(os.PathSeparator)) || !strings.EqualFold(filepath.Base(profile), "EBWebView") {
			return nil, ddCookieError()
		}
		if e = ddVerifiedPublisher(ctx, image, "Microsoft Corporation"); e != nil {
			return nil, e
		}
		netCreated, e := ddProcessCreated(int(row.pid))
		if e != nil {
			return nil, ddCookieError()
		}
		if selected != nil {
			return nil, capability("publish.dd_desktop", "DD does not expose one unique active browser session")
		}
		selected = &ddDesktop{guiPID: int(gui.pid), networkPID: int(row.pid), guiCreated: created, networkCreated: netCreated, installation: installation, networkImage: image, profile: profile}
	}
	if selected == nil {
		return nil, ddCookieError()
	}
	return selected, nil
}
func (d *ddDesktop) verify(ctx context.Context) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	gui, err := ddProcessCreated(d.guiPID)
	if err != nil || gui != d.guiCreated {
		return ddCookieError()
	}
	network, err := ddProcessCreated(d.networkPID)
	if err != nil || network != d.networkCreated {
		return ddCookieError()
	}
	image, args, err := ddProcessImage(uint32(d.networkPID), true)
	if err != nil || !strings.EqualFold(image, d.networkImage) {
		return ddCookieError()
	}
	profiles := 0
	for _, arg := range args {
		if value, ok := strings.CutPrefix(arg, "--user-data-dir="); ok {
			profile, e := filepath.EvalSymlinks(value)
			if e != nil || !strings.EqualFold(profile, d.profile) {
				return ddCookieError()
			}
			profiles++
		}
	}
	if profiles != 1 {
		return ddCookieError()
	}
	return nil
}
func (d *ddDesktop) cookie(ctx context.Context) (string, error) {
	if err := d.verify(ctx); err != nil {
		return "", err
	}
	files, err := ddCookieFiles(ctx, d.networkPID, d.profile)
	if err != nil {
		return "", err
	}
	defer func() {
		for _, v := range files {
			clear(v)
		}
	}()
	f, err := os.Open(filepath.Join(d.profile, "Local State"))
	if err != nil {
		return "", ddCookieError()
	}
	state, err := io.ReadAll(io.LimitReader(f, (4<<20)+1))
	f.Close()
	if err != nil {
		return "", ddCookieError()
	}
	cookie, err := ddCookieFromSnapshot(ctx, files["cookies"], files["cookies-wal"], state, ddUnprotect)
	clear(state)
	if err != nil {
		return "", err
	}
	if err = d.verify(ctx); err != nil {
		return "", err
	}
	return cookie, nil
}
func ddCookieFiles(ctx context.Context, pid int, profile string) (map[string][]byte, error) {
	var previous map[string][]byte
	for i := 0; i < 3; i++ {
		current, err := ddCookieFilesOnce(ctx, pid, profile)
		if err != nil {
			return nil, err
		}
		same := previous != nil && len(current) == len(previous)
		for key, value := range current {
			if !bytes.Equal(value, previous[key]) {
				same = false
			}
		}
		for _, v := range previous {
			clear(v)
		}
		if same {
			return current, nil
		}
		previous = current
	}
	for _, v := range previous {
		clear(v)
	}
	return nil, capability("publish.dd_desktop_cookie", "DD browser cookie files changed during the bounded snapshot")
}
func ddCookieFilesOnce(ctx context.Context, pid int, profile string) (map[string][]byte, error) {
	process, err := windows.OpenProcess(windows.PROCESS_QUERY_INFORMATION|windows.PROCESS_DUP_HANDLE, false, uint32(pid))
	if err != nil {
		return nil, ddCookieError()
	}
	defer windows.CloseHandle(process)
	type handleEntry struct {
		Handle, HandleCount, PointerCount  uintptr
		Access, Type, Attributes, Reserved uint32
	}
	var raw []byte
	needed := uint32(65536)
	for i := 0; i < 4; i++ {
		if needed > 8<<20 {
			return nil, ddCookieError()
		}
		raw = make([]byte, needed)
		err = windows.NtQueryInformationProcess(process, 51, unsafe.Pointer(&raw[0]), uint32(len(raw)), &needed)
		if err == nil {
			break
		}
		if needed <= uint32(len(raw)) {
			needed = uint32(len(raw)) * 2
		}
	}
	if err != nil {
		return nil, ddCookieError()
	}
	count := *(*uintptr)(unsafe.Pointer(&raw[0]))
	entrySize := unsafe.Sizeof(handleEntry{})
	if count > uintptr(len(raw)-16)/entrySize {
		return nil, ddCookieError()
	}
	expected := strings.ToLower(filepath.Clean(filepath.Join(profile, "Default", "Network", "Cookies")))
	files := map[string][]byte{}
	for i := uintptr(0); i < count; i++ {
		if err = ctx.Err(); err != nil {
			return nil, err
		}
		entry := (*handleEntry)(unsafe.Pointer(&raw[16+i*entrySize]))
		if entry.Access&1 == 0 {
			continue
		}
		var duplicate windows.Handle
		if windows.DuplicateHandle(process, windows.Handle(entry.Handle), windows.CurrentProcess(), &duplicate, 0, false, windows.DUPLICATE_SAME_ACCESS) != nil {
			continue
		}
		name, data := ddReadCookieHandle(duplicate, expected)
		windows.CloseHandle(duplicate)
		if name != "" {
			if existing, ok := files[name]; ok && !bytes.Equal(existing, data) {
				clear(data)
				return nil, ddCookieError()
			}
			files[name] = data
		}
	}
	if len(files["cookies"]) == 0 {
		return nil, ddCookieError()
	}
	return files, nil
}
func ddReadCookieHandle(handle windows.Handle, expected string) (string, []byte) {
	typ, err := windows.GetFileType(handle)
	if err != nil || typ != windows.FILE_TYPE_DISK {
		return "", nil
	}
	path := make([]uint16, 32768)
	n, err := windows.GetFinalPathNameByHandle(handle, &path[0], uint32(len(path)), 0)
	if err != nil || n == 0 || n >= uint32(len(path)) {
		return "", nil
	}
	name := strings.ToLower(filepath.Clean(strings.TrimPrefix(windows.UTF16ToString(path[:n]), `\\?\`)))
	key := ""
	if name == expected {
		key = "cookies"
	} else if name == expected+"-wal" {
		key = "cookies-wal"
	} else {
		return "", nil
	}
	var info windows.ByHandleFileInformation
	if windows.GetFileInformationByHandle(handle, &info) != nil {
		return "", nil
	}
	size := uint64(info.FileSizeHigh)<<32 | uint64(info.FileSizeLow)
	if size == 0 || size > 16<<20 {
		return "", nil
	}
	mapping, err := windows.CreateFileMapping(handle, nil, windows.PAGE_READONLY, 0, 0, nil)
	if err != nil {
		return "", nil
	}
	defer windows.CloseHandle(mapping)
	view, err := windows.MapViewOfFile(mapping, windows.FILE_MAP_READ, 0, 0, uintptr(size))
	if err != nil {
		return "", nil
	}
	defer windows.UnmapViewOfFile(view)
	data := make([]byte, int(size))
	if ddNativeRead(view, data) != nil {
		return "", nil
	}
	return key, data
}
