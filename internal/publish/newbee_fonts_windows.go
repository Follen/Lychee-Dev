//go:build windows && amd64

package publish

import (
	"context"
	"encoding/binary"
	"encoding/json"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"sort"
	"strconv"
	"strings"
	"time"
	"unsafe"

	"golang.org/x/sys/windows"
	"golang.org/x/sys/windows/registry"
)

func newBeeInstalledArchive() (string, error) {
	candidates := map[string]bool{}
	for _, hive := range []registry.Key{registry.CURRENT_USER, registry.LOCAL_MACHINE} {
		for _, view := range []uint32{registry.WOW64_64KEY, registry.WOW64_32KEY} {
			key, err := registry.OpenKey(hive, `Software\Microsoft\Windows\CurrentVersion\Uninstall`, registry.READ|view)
			if err != nil {
				continue
			}
			names, _ := key.ReadSubKeyNames(-1)
			for _, name := range names {
				sub, err := registry.OpenKey(key, name, registry.READ|view)
				if err != nil {
					continue
				}
				display, _, _ := sub.GetStringValue("DisplayName")
				if strings.HasPrefix(display, "NewBeeBox") {
					for _, field := range []string{"InstallLocation", "DisplayIcon", "UninstallString"} {
						value, _, _ := sub.GetStringValue(field)
						value = strings.TrimSpace(value)
						if value == "" {
							continue
						}
						if strings.HasPrefix(value, `"`) {
							value = strings.SplitN(value[1:], `"`, 2)[0]
						} else if end := strings.Index(strings.ToLower(value), ".exe"); end >= 0 {
							value = value[:end+4]
						}
						if field != "InstallLocation" {
							value = filepath.Dir(value)
						}
						candidates[value] = true
					}
				}
				sub.Close()
			}
			key.Close()
		}
	}
	dirs := []string{}
	for dir := range candidates {
		dirs = append(dirs, dir)
	}
	sort.Strings(dirs)
	for _, dir := range dirs {
		exe, err := newBeeLocal(dir, "NewBeeBox.exe")
		if err != nil {
			continue
		}
		archive, err := newBeeLocal(dir, "resources", "app.asar")
		if err != nil {
			continue
		}
		if newBeeVerifyPublisher(exe) == nil {
			return archive, nil
		}
	}
	return "", capability("publish.newbee_native", "publisher-verified NewBeeBox desktop installation is required for native font upload")
}
func newBeeVerifyPublisher(path string) error {
	utf16, err := windows.UTF16PtrFromString(path)
	if err != nil {
		return err
	}
	info := windows.WinTrustFileInfo{Size: uint32(unsafe.Sizeof(windows.WinTrustFileInfo{})), FilePath: utf16}
	data := windows.WinTrustData{Size: uint32(unsafe.Sizeof(windows.WinTrustData{})), UIChoice: windows.WTD_UI_NONE, RevocationChecks: windows.WTD_REVOKE_NONE, UnionChoice: windows.WTD_CHOICE_FILE, StateAction: windows.WTD_STATEACTION_VERIFY, FileOrCatalogOrBlobOrSgnrOrCert: unsafe.Pointer(&info)}
	err = windows.WinVerifyTrustEx(windows.InvalidHWND, &windows.WINTRUST_ACTION_GENERIC_VERIFY_V2, &data)
	defer func() {
		data.StateAction = windows.WTD_STATEACTION_CLOSE
		_ = windows.WinVerifyTrustEx(windows.InvalidHWND, &windows.WINTRUST_ACTION_GENERIC_VERIFY_V2, &data)
	}()
	if err != nil {
		return capability("publish.newbee_native", "NewBeeBox executable signature is invalid")
	}
	var store, message windows.Handle
	if err = windows.CryptQueryObject(windows.CERT_QUERY_OBJECT_FILE, unsafe.Pointer(utf16), windows.CERT_QUERY_CONTENT_FLAG_PKCS7_SIGNED_EMBED, windows.CERT_QUERY_FORMAT_FLAG_BINARY, 0, nil, nil, nil, &store, &message, nil); err != nil {
		return capability("publish.newbee_native", "embedded signer is unavailable")
	}
	defer windows.CertCloseStore(store, 0)
	crypt := windows.NewLazySystemDLL("crypt32.dll")
	defer crypt.NewProc("CryptMsgClose").Call(uintptr(message))
	get := crypt.NewProc("CryptMsgGetParam")
	var size uint32
	// CMSG_SIGNER_INFO_PARAM returns an owned buffer with issuer and serial blobs.
	ok, _, _ := get.Call(uintptr(message), 6, 0, 0, uintptr(unsafe.Pointer(&size)))
	if ok == 0 || size < 48 || size > 1<<20 {
		return capability("publish.newbee_native", "embedded signer information is invalid")
	}
	buffer := make([]byte, size)
	ok, _, _ = get.Call(uintptr(message), 6, 0, uintptr(unsafe.Pointer(&buffer[0])), uintptr(unsafe.Pointer(&size)))
	if ok == 0 {
		return capability("publish.newbee_native", "embedded signer information is unavailable")
	}
	type signerInfo struct {
		Version uint32
		Issuer  windows.CertNameBlob
		Serial  windows.CryptIntegerBlob
	}
	signer := (*signerInfo)(unsafe.Pointer(&buffer[0]))
	find := windows.CertInfo{Issuer: signer.Issuer, SerialNumber: signer.Serial}
	cert, err := windows.CertFindCertificateInStore(store, windows.X509_ASN_ENCODING|windows.PKCS_7_ASN_ENCODING, 0, windows.CERT_FIND_SUBJECT_CERT, unsafe.Pointer(&find), nil)
	if err != nil {
		return capability("publish.newbee_native", "signer certificate is unavailable")
	}
	defer windows.CertFreeCertificateContext(cert)
	count := windows.CertGetNameString(cert, windows.CERT_NAME_SIMPLE_DISPLAY_TYPE, 0, nil, nil, 0)
	if count == 0 || count > 4096 {
		return capability("publish.newbee_native", "signature publisher is unavailable")
	}
	name := make([]uint16, count)
	windows.CertGetNameString(cert, windows.CERT_NAME_SIMPLE_DISPLAY_TYPE, 0, nil, &name[0], count)
	if windows.UTF16ToString(name) != "南宁新手村科技有限公司" {
		return capability("publish.newbee_native", "NewBeeBox publisher does not match")
	}
	return nil
}
func newBeeExtractBinding(archive, destination string) (string, error) {
	file, err := os.Open(archive)
	if err != nil {
		return "", err
	}
	defer file.Close()
	header := make([]byte, 16)
	if _, err = io.ReadFull(file, header); err != nil {
		return "", err
	}
	headerSize := binary.LittleEndian.Uint32(header[4:8])
	jsonSize := binary.LittleEndian.Uint32(header[12:16])
	if jsonSize < 2 || jsonSize > 16<<20 {
		return "", capability("publish.newbee_native", "installed app archive has an invalid header")
	}
	raw := make([]byte, jsonSize)
	if _, err = io.ReadFull(file, raw); err != nil {
		return "", err
	}
	var object map[string]any
	decoder := json.NewDecoder(strings.NewReader(string(raw)))
	decoder.UseNumber()
	if err = decoder.Decode(&object); err != nil {
		return "", err
	}
	walk := func(node map[string]any, name string) map[string]any {
		files, _ := node["files"].(map[string]any)
		entry, _ := files[name].(map[string]any)
		return entry
	}
	entry := walk(walk(object, "node_modules"), "nbb-core")
	if dist := walk(entry, "dist"); dist != nil {
		entry = dist
	}
	entry = walk(entry, "nbb-core.win32-x64-msvc.node")
	size := newBeeID(entry["size"])
	offset, _ := strconv.ParseInt(String(entry, "offset"), 10, 64)
	if size <= 0 || size > 128<<20 || offset < 0 || entry["unpacked"] == true {
		return "", capability("publish.newbee_native", "installed native uploader is unavailable")
	}
	if _, err = file.Seek(8+int64(headerSize)+offset, io.SeekStart); err != nil {
		return "", err
	}
	raw = make([]byte, size)
	if _, err = io.ReadFull(file, raw); err != nil {
		return "", err
	}
	path := filepath.Join(destination, "nbb-core.win32-x64-msvc.node")
	return path, os.WriteFile(path, raw, 0600)
}
func newBeeUploadFonts(ctx context.Context, path string) (string, error) {
	archive, err := newBeeInstalledArchive()
	if err != nil {
		return "", err
	}
	node, err := exec.LookPath("node")
	if err != nil {
		return "", capability("publish.newbee_native", "native uploader requires Node.js")
	}
	dir, err := os.MkdirTemp("", "lycheedev-newbee-font-")
	if err != nil {
		return "", err
	}
	defer os.RemoveAll(dir)
	binding, err := newBeeExtractBinding(archive, dir)
	if err != nil {
		return "", err
	}
	const script = `const fs=require('node:fs');const input=JSON.parse(fs.readFileSync(0,'utf8'));(async()=>{try{const core=require(input.binding);if(typeof core.setLogCallback==='function')core.setLogCallback(()=>{});const uploader=new core.OssUploader(JSON.stringify({apiBase:'https://api.next.newbeebox.com/uploadserver'}));const r=await uploader.uploadFolder(JSON.stringify({path:input.path}));if(!r||r.code!==1||typeof r.data!=='string')throw Error();process.stdout.write('\nLYCHEEDEV_FONT:'+JSON.stringify({index:r.data})+'\n')}catch(_){process.exitCode=1}})();`
	raw, _ := json.Marshal(map[string]any{"binding": binding, "path": path})
	ctx, cancel := context.WithTimeout(ctx, 10*time.Minute)
	defer cancel()
	cmd := exec.CommandContext(ctx, node, "-e", script)
	cmd.Stdin = strings.NewReader(string(raw))
	var output newBeeOutput
	cmd.Stdout = &output
	cmd.Stderr = io.Discard
	if err = cmd.Run(); err != nil {
		if ctx.Err() != nil {
			return "", ctx.Err()
		}
		return "", failure("publish.newbee_native", "native font upload is uncertain")
	}
	if output.exceeded {
		return "", failure("publish.newbee_native", "native font response exceeded limit")
	}
	for _, line := range strings.Split(output.String(), "\n") {
		if strings.HasPrefix(line, "LYCHEEDEV_FONT:") {
			var response map[string]string
			if json.Unmarshal([]byte(strings.TrimPrefix(line, "LYCHEEDEV_FONT:")), &response) == nil && regexp.MustCompile(`^[A-Za-z0-9_-]{1,256}$`).MatchString(response["index"]) {
				return response["index"], nil
			}
		}
	}
	return "", failure("publish.newbee_native", "native font upload did not return an object index")
}

func newBeeFontReady() error {
	if _, err := newBeeInstalledArchive(); err != nil {
		return err
	}
	if _, err := exec.LookPath("node"); err != nil {
		return capability("publish.newbee_native", "native uploader requires Node.js")
	}
	return nil
}
