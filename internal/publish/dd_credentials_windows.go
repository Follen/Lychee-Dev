//go:build windows && amd64

package publish

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"golang.org/x/sys/windows"
	"golang.org/x/sys/windows/registry"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"syscall"
	"unsafe"
)

func ddUnprotect(cipher, entropy []byte) ([]byte, error) {
	if len(cipher) == 0 {
		return nil, ddCredentialError()
	}
	in := windows.DataBlob{Size: uint32(len(cipher)), Data: &cipher[0]}
	var out windows.DataBlob
	var extra *windows.DataBlob
	if len(entropy) > 0 {
		extra = &windows.DataBlob{Size: uint32(len(entropy)), Data: &entropy[0]}
	}
	if windows.CryptUnprotectData(&in, nil, extra, 0, nil, windows.CRYPTPROTECT_UI_FORBIDDEN, &out) != nil {
		return nil, ddCredentialError()
	}
	defer windows.LocalFree(windows.Handle(unsafe.Pointer(out.Data)))
	if out.Size == 0 || out.Size > 131072 {
		return nil, ddCredentialError()
	}
	return append([]byte(nil), unsafe.Slice(out.Data, int(out.Size))...), nil
}
func ddRoaming() (string, error) {
	return windows.KnownFolderPath(windows.FOLDERID_RoamingAppData, windows.KF_FLAG_DEFAULT)
}
func ddLoadCredential(ctx context.Context) (ddCredential, error) {
	root, err := ddRoaming()
	if err != nil {
		return ddCredential{}, ddCredentialError()
	}
	return ddReadCredential(ctx, filepath.Join(root, "CCVoiceHub", "cred.db"), ddUnprotect)
}
func ddVerifiedPublisher(ctx context.Context, path, publisher string) error {
	system, err := windows.GetSystemDirectory()
	if err != nil {
		return ddCredentialError()
	}
	cmd := exec.CommandContext(ctx, filepath.Join(system, "WindowsPowerShell", "v1.0", "powershell.exe"), "-NoProfile", "-NonInteractive", "-Command", `$s=Get-AuthenticodeSignature -LiteralPath $env:LYCHEEDEV_DD_IMAGE; if($s.Status -ne 'Valid' -or $s.SignerCertificate.GetNameInfo([System.Security.Cryptography.X509Certificates.X509NameType]::SimpleName,$false) -ne $env:LYCHEEDEV_DD_PUBLISHER){exit 1}`)
	cmd.Env = append(os.Environ(), "LYCHEEDEV_DD_IMAGE="+path, "LYCHEEDEV_DD_PUBLISHER="+publisher)
	cmd.SysProcAttr = &syscall.SysProcAttr{HideWindow: true, CreationFlags: 0x08000000}
	if cmd.Run() != nil {
		return capability("publish.dd_installation", "DD component does not have its required valid publisher signature")
	}
	return nil
}
func ddInstallation(ctx context.Context, selected string) (string, error) {
	candidates := []string{}
	if selected != "" {
		if !filepath.IsAbs(selected) {
			return "", invalid("$.installation", "DD installation must be an absolute directory")
		}
		candidates = append(candidates, selected, filepath.Join(selected, "100130"))
	} else {
		if rows, e := ddProcessRows(); e == nil {
			for _, row := range rows {
				if strings.EqualFold(row.name, "netease_dd.exe") {
					if image, _, e := ddProcessImage(row.pid, false); e == nil {
						candidates = append(candidates, filepath.Dir(image))
					}
				}
			}
		}
		for _, root := range []registry.Key{registry.CURRENT_USER, registry.LOCAL_MACHINE} {
			for _, view := range []uint32{registry.WOW64_64KEY, registry.WOW64_32KEY} {
				key, err := registry.OpenKey(root, `SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall`, registry.READ|view)
				if err != nil {
					continue
				}
				names, _ := key.ReadSubKeyNames(-1)
				for _, name := range names {
					sub, e := registry.OpenKey(key, name, registry.READ|view)
					if e != nil {
						continue
					}
					display, _, _ := sub.GetStringValue("DisplayName")
					if normalized := strings.ToLower(strings.ReplaceAll(display, " ", "")); strings.Contains(normalized, "网易dd") || strings.Contains(normalized, "neteasedd") || strings.Contains(normalized, "ccvoicehub") {
						for _, field := range []string{"InstallLocation", "DisplayIcon"} {
							dir, _, _ := sub.GetStringValue(field)
							dir = strings.Trim(strings.TrimSpace(dir), "\"")
							if field == "DisplayIcon" {
								if at := strings.LastIndex(dir, ","); at >= 0 {
									dir = dir[:at]
								}
								dir = strings.Trim(strings.TrimSpace(dir), "\"")
								if strings.EqualFold(filepath.Ext(dir), ".exe") {
									dir = filepath.Dir(dir)
								}
							}
							if filepath.IsAbs(dir) {
								candidates = append(candidates, dir, filepath.Join(dir, "100130"))
							}
						}
					}
					sub.Close()
				}
				key.Close()
			}
		}
	}
	found := ""
	seen := map[string]bool{}
	for _, dir := range candidates {
		if err := ctx.Err(); err != nil {
			return "", err
		}
		resolved, err := filepath.EvalSymlinks(dir)
		if err != nil || seen[strings.ToLower(resolved)] {
			continue
		}
		seen[strings.ToLower(resolved)] = true
		if _, err = os.Stat(filepath.Join(resolved, "netease_dd.exe")); err != nil {
			continue
		}
		f, e := ddPinSDK(filepath.Join(resolved, "libnep.dll"), ddNEPSHA256)
		if e != nil {
			continue
		}
		f.Close()
		if err = ddVerifiedPublisher(ctx, filepath.Join(resolved, "netease_dd.exe"), "NetEase (Hangzhou) Network Co., Ltd"); err != nil {
			return "", err
		}
		if found != "" && !strings.EqualFold(found, resolved) {
			return "", invalid("$.installation", "multiple supported DD installations; select one absolute directory")
		}
		found = resolved
	}
	if found == "" {
		return "", capability("publish.dd_installation", "DD 100130 installation is unavailable; select its program directory")
	}
	return found, nil
}
func ddPinSDK(path, digest string) (*os.File, error) {
	f, err := openArtifact(path)
	if err != nil {
		return nil, capability("publish.dd_sdk", "required DD SDK file is unavailable")
	}
	sum := sha256.New()
	n, err := io.Copy(sum, io.LimitReader(f, 64<<20+1))
	if err != nil || n > 64<<20 || hex.EncodeToString(sum.Sum(nil)) != digest {
		f.Close()
		return nil, capability("publish.dd_sdk", "DD SDK bytes differ from the pinned 100130 component")
	}
	return f, nil
}
func ddAppID(ctx context.Context, dir string) (string, error) {
	if err := ctx.Err(); err != nil {
		return "", err
	}
	file, err := ddPinSDK(filepath.Join(dir, "Unifiedlogin.dll"), "cbb16ca38f679e4d004e099e52e0374d19961baa854bde551797f30c22b926f9")
	if err != nil {
		return "", err
	}
	defer file.Close()
	names := []string{"ursk1.dat", "ursk2.dat", "ursk3.dat"}
	hashes := []string{"30a4e4f8ea3c7aec3f9c2394abebf709d196ea99e5a3c0e75f45d9908a01da7b", "00b6b0925372f956904a40aa503639dd499b40b26d29b0ac6abe9dc138ef0299", "899f232085ba6cba233c4d6da5e211d22f6a1c0a84092a90340a2f5cb9458455"}
	values := []string{"cc_team", "1.1.30", "3822d0cbc13db710cb04d9e4025eb3d2"}
	for i, name := range names {
		path := filepath.Join(dir, "resource", name)
		f, e := ddPinSDK(path, hashes[i])
		if e != nil {
			return "", e
		}
		defer f.Close()
		values = append(values, path)
	}
	module, err := windows.LoadLibraryEx(filepath.Join(dir, "Unifiedlogin.dll"), 0, windows.LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR|windows.LOAD_LIBRARY_SEARCH_SYSTEM32)
	if err != nil {
		return "", capability("publish.dd_urs_sdk", "cannot load pinned DD login SDK")
	}
	defer windows.FreeLibrary(module)
	loaded := make([]uint16, 32768)
	n, e := windows.GetModuleFileName(module, &loaded[0], uint32(len(loaded)))
	if e != nil || n == 0 || n >= uint32(len(loaded)) {
		return "", capability("publish.dd_urs_sdk", "loaded SDK image identity is unavailable")
	}
	actual, e := filepath.EvalSymlinks(windows.UTF16ToString(loaded[:n]))
	expected, e2 := filepath.EvalSymlinks(filepath.Join(dir, "Unifiedlogin.dll"))
	if e != nil || e2 != nil || !strings.EqualFold(actual, expected) {
		return "", capability("publish.dd_urs_sdk", "loaded SDK image differs from the selected installation")
	}
	factory, err := windows.GetProcAddress(module, "CreateSDKInterface")
	if err != nil {
		return "", capability("publish.dd_urs_sdk", "login SDK factory is absent")
	}
	appID, err := ddInvokeAppID(factory, values)
	if err != nil {
		return "", err
	}
	if err = ctx.Err(); err != nil {
		return "", err
	}
	return appID, nil
}

func ddInvokeAppID(factory uintptr, values []string) (string, error) {
	if len(values) != 6 {
		return "", ddCCProtocol()
	}
	object, _, _ := syscall.SyscallN(factory)
	if object == 0 {
		return "", capability("publish.dd_urs_sdk", "login SDK object is unavailable")
	}
	table, err := ddSDKTable(object, 6)
	if err != nil {
		return "", err
	}
	defer syscall.SyscallN(table[2], object)
	params := [6]*byte{}
	for i, s := range values {
		b, e := windows.BytePtrFromString(s)
		if e != nil {
			return "", ddCCProtocol()
		}
		params[i] = b
	}
	code, _, _ := syscall.SyscallN(table[5], object, uintptr(unsafe.Pointer(&params)))
	runtime.KeepAlive(params)
	if code != 0 {
		return "", capability("publish.dd_urs_sdk", "login SDK initialization failed")
	}
	method, _, _ := syscall.SyscallN(table[4], object, 2)
	if method == 0 {
		return "", capability("publish.dd_urs_sdk", "appId SDK method is unavailable")
	}
	mt, err := ddSDKTable(method, 4)
	if err != nil {
		return "", err
	}
	defer syscall.SyscallN(mt[1], method)
	var output, message *byte
	code, _, _ = syscall.SyscallN(mt[3], method, uintptr(unsafe.Pointer(&output)), uintptr(unsafe.Pointer(&message)))
	for _, p := range []*byte{output, message} {
		if p != nil {
			defer syscall.SyscallN(table[3], object, uintptr(unsafe.Pointer(p)))
		}
	}
	if code != 0 || output == nil {
		return "", capability("publish.dd_urs_sdk", "appId SDK request failed")
	}
	raw := []byte{}
	for i := 0; i <= 65536; i++ {
		b := *(*byte)(unsafe.Add(unsafe.Pointer(output), i))
		if b == 0 {
			break
		}
		raw = append(raw, b)
	}
	if len(raw) > 65536 {
		return "", ddCCProtocol()
	}
	var value map[string]any
	if ddJSON(raw, &value) != nil {
		return "", ddCCProtocol()
	}
	appID, _ := value["appId"].(string)
	if strings.TrimSpace(appID) == "" || len(appID) > 4096 || strings.ContainsRune(appID, 0) {
		return "", ddCCProtocol()
	}
	return appID, nil
}

// SDK objects and mapped views are native allocations. ReadProcessMemory copies
// their bounded bytes without presenting an integer address as a Go pointer.
func ddNativeRead(address uintptr, target []byte) error {
	if len(target) == 0 {
		return nil
	}
	var copied uintptr
	if windows.ReadProcessMemory(windows.CurrentProcess(), address, &target[0], uintptr(len(target)), &copied) != nil || copied != uintptr(len(target)) {
		return capability("publish.dd_native_read", "DD native object could not be read")
	}
	return nil
}
func ddSDKTable(object uintptr, count int) ([]uintptr, error) {
	var address uintptr
	if err := ddNativeRead(object, unsafe.Slice((*byte)(unsafe.Pointer(&address)), int(unsafe.Sizeof(address)))); err != nil {
		return nil, err
	}
	table := make([]uintptr, count)
	if err := ddNativeRead(address, unsafe.Slice((*byte)(unsafe.Pointer(&table[0])), count*int(unsafe.Sizeof(address)))); err != nil {
		return nil, err
	}
	return table, nil
}
