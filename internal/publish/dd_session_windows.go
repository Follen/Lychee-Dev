//go:build windows && amd64

package publish

import (
	"context"
	"encoding/json"
	"fmt"
	"net"
	"net/url"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"sync"
	"syscall"

	"github.com/Microsoft/go-winio"
	"github.com/follenfang/lycheedev/internal/vault"
	"golang.org/x/sys/windows"
)

func ddPipeName(id string) string { return `\\.\pipe\lycheedev-publish-` + id }
func ddUserSDDL() (string, error) {
	user, err := windows.GetCurrentProcessToken().GetTokenUser()
	if err != nil {
		return "", err
	}
	return "D:P(A;;GA;;;" + user.User.Sid.String() + ")", nil
}
func ddPrivateDirectory(path string) error {
	if err := os.MkdirAll(path, 0700); err != nil {
		return err
	}
	sddl, err := ddUserSDDL()
	if err != nil {
		return err
	}
	sd, err := windows.SecurityDescriptorFromString(strings.Replace(sddl, ";;GA;", ";OICI;FA;", 1))
	if err != nil {
		return err
	}
	dacl, _, err := sd.DACL()
	if err != nil {
		return err
	}
	return windows.SetNamedSecurityInfo(path, windows.SE_FILE_OBJECT, windows.DACL_SECURITY_INFORMATION|windows.PROTECTED_DACL_SECURITY_INFORMATION, nil, nil, dacl, nil)
}
func ddConfigureWorker(cmd *exec.Cmd) {
	cmd.SysProcAttr = &syscall.SysProcAttr{HideWindow: true, CreationFlags: 0x08000000}
}
func ddProcessCreated(pid int) (uint64, error) {
	h, err := windows.OpenProcess(windows.PROCESS_QUERY_LIMITED_INFORMATION|windows.SYNCHRONIZE, false, uint32(pid))
	if err != nil {
		return 0, err
	}
	defer windows.CloseHandle(h)
	wait, err := windows.WaitForSingleObject(h, 0)
	if err != nil || wait != uint32(windows.WAIT_TIMEOUT) {
		return 0, ddCCLost()
	}
	var created, exit, kernel, user windows.Filetime
	if err = windows.GetProcessTimes(h, &created, &exit, &kernel, &user); err != nil {
		return 0, err
	}
	return uint64(created.HighDateTime)<<32 | uint64(created.LowDateTime), nil
}
func ddWorkerAlive(state ddSessionState) (bool, error) {
	if state.PID <= 0 || state.Created == 0 {
		return false, nil
	}
	created, err := ddProcessCreated(state.PID)
	if err != nil {
		if err == windows.ERROR_INVALID_PARAMETER || err == windows.ERROR_NOT_FOUND {
			return false, nil
		}
		var e *Error
		if errorsAsDD(err, &e) && e.Code == "publish.dd_cc_lost" {
			return false, nil
		}
		return false, err
	}
	return created == state.Created, nil
}
func errorsAsDD(err error, out **Error) bool {
	v, ok := err.(*Error)
	if ok {
		*out = v
	}
	return ok
}
func ddListenSession(state ddSessionState) (net.Listener, error) {
	sddl, err := ddUserSDDL()
	if err != nil {
		return nil, err
	}
	return winio.ListenPipe(state.Pipe, &winio.PipeConfig{SecurityDescriptor: sddl, InputBufferSize: 65536, OutputBufferSize: 65536})
}
func ddDialSession(ctx context.Context, state ddSessionState) (net.Conn, error) {
	alive, err := ddWorkerAlive(state)
	if err != nil || !alive {
		return nil, ddCCLost()
	}
	conn, err := winio.DialPipeContext(ctx, state.Pipe)
	if err != nil {
		return nil, err
	}
	handle, ok := conn.(interface{ Fd() uintptr })
	var pid uint32
	if !ok || windows.GetNamedPipeServerProcessId(windows.Handle(handle.Fd()), &pid) != nil || int(pid) != state.PID {
		conn.Close()
		return nil, ddCCLost()
	}
	created, err := ddProcessCreated(int(pid))
	if err != nil || created != state.Created {
		conn.Close()
		return nil, ddCCLost()
	}
	return conn, nil
}

type ddTaskSession struct {
	transport  *ddTransport
	cc         *ddCC
	desktop    *ddDesktop
	ident      map[string]any
	once       sync.Once
	closed     bool
	closeError error
	release    func()
}

func (s *ddTaskSession) check(ctx context.Context) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if s.closed {
		return ddCCLost()
	}
	if s.cc != nil {
		return s.cc.active()
	}
	if s.desktop != nil {
		return s.desktop.verify(ctx)
	}
	return ddCCLost()
}
func (s *ddTaskSession) call(ctx context.Context, path string, q url.Values, body any, mutation bool) (map[string]any, error) {
	if err := s.check(ctx); err != nil {
		return nil, err
	}
	return s.transport.call(ctx, path, q, body, mutation)
}
func (s *ddTaskSession) channels(ctx context.Context) (any, error) {
	if err := s.check(ctx); err != nil {
		return nil, err
	}
	if s.cc != nil {
		return s.cc.channels(ctx)
	}
	return s.desktop.channelsWithElevation(ctx)
}
func (s *ddTaskSession) identity() map[string]any {
	out := map[string]any{}
	for k, v := range s.ident {
		out[k] = v
	}
	out["active"] = s.check(context.Background()) == nil
	return out
}
func (s *ddTaskSession) close() error {
	s.once.Do(func() {
		s.closed = true
		if s.transport != nil {
			s.closeError = s.transport.close()
		}
		if s.cc != nil {
			s.cc.close()
		}
		if s.release != nil {
			s.release()
		}
	})
	return s.closeError
}

func ddSessionLease(ctx context.Context) (func(), error) {
	root, err := ddRoaming()
	if err != nil {
		return nil, err
	}
	dir := filepath.Join(root, "LycheeDev", "publish", "dd")
	if err = ddPrivateDirectory(dir); err != nil {
		return nil, err
	}
	lease, err := vault.TryAcquireLease(ctx, dir, "task-runtime")
	if err != nil {
		return nil, capability("publish.dd_session_busy", "an existing DD task owns the runtime; reuse or close its original session")
	}
	return func() { lease.Close() }, nil
}
func ddOpenTaskSession(ctx context.Context, home, installation string) (ddAPI, error) {
	desktop, err := ddSelectDesktop(ctx, installation)
	if err != nil {
		return nil, err
	}
	session := &ddTaskSession{desktop: desktop, ident: map[string]any{}}
	session.release, err = ddSessionLease(ctx)
	if err != nil {
		return nil, err
	}
	good := false
	defer func() {
		if !good {
			session.close()
		}
	}()
	cookie := ""
	if desktop != nil {
		cookie, err = desktop.cookie(ctx)
		if err != nil {
			return nil, err
		}
		session.ident["source"] = "desktop-cookie"
		session.ident["native_login_count"] = 0
		session.ident["gui_pid"] = desktop.guiPID
		session.ident["gui_created"] = desktop.guiCreated
	} else {
		credential, e := ddLoadCredential(ctx)
		if e != nil {
			return nil, e
		}
		device, e := ddDevice(ctx, home)
		if e != nil {
			return nil, e
		}
		appID, e := ddAppID(ctx, installation)
		if e != nil {
			return nil, e
		}
		if desktop, e = ddSelectDesktop(ctx, installation); e != nil {
			return nil, e
		} else if desktop != nil {
			return nil, capability("publish.dd_desktop", "DD GUI started during preparation; native login was not attempted")
		}
		session.cc, err = ddOpenCC(ctx, credential, appID, device)
		credential.token = ""
		if err != nil {
			return nil, err
		}
		cookie = session.cc.cookie
		session.ident["source"] = "native-login"
		session.ident["native_login_count"] = 1
		session.ident["cc_uid"] = session.cc.uid
		session.ident["cc_eid"] = session.cc.eid
		if _, err = session.cc.jwt(ctx, NewHTTP()); err != nil {
			return nil, err
		}
	}
	signer, err := openDDSigner(ctx, filepath.Join(installation, "libnep.dll"))
	if err != nil {
		return nil, err
	}
	session.transport = &ddTransport{http: NewHTTP(), signer: signer, cookie: cookie, origin: ddOrigin}
	value, err := session.transport.call(ctx, "/login/dflogin", nil, nil, false)
	if err != nil {
		return nil, err
	}
	data, _ := value["result"].(map[string]any)
	token, _ := data["token"].(string)
	if strings.TrimSpace(token) == "" || len(token) > 65536 || strings.ContainsAny(token, "\r\n\x00") {
		return nil, ddCCProtocol()
	}
	id, err := ddIdentityText(data["uid"])
	if err != nil {
		return nil, ddCCProtocol()
	}
	session.transport.token = token
	session.ident["author_id"] = id
	session.ident["nickname"], _ = data["nickname"].(string)
	for _, value := range session.ident {
		if text, ok := value.(string); ok && (strings.Contains(text, token) || strings.Contains(text, cookie)) {
			return nil, ddCCProtocol()
		}
	}
	session.ident["logout_requested"] = false
	if err = session.check(ctx); err != nil {
		return nil, err
	}
	good = true
	return session, nil
}
func ddDevice(ctx context.Context, home string) (ddCCDevice, error) {
	var out ddCCDevice
	dir := filepath.Join(home, "publish", "dd")
	if err := ddPrivateDirectory(dir); err != nil {
		return out, err
	}
	lease, err := vault.AcquireLease(ctx, dir, "device")
	if err != nil {
		return out, err
	}
	defer lease.Close()
	path := filepath.Join(dir, "device.json")
	raw, err := os.ReadFile(path)
	if err == nil {
		var value struct {
			ClientNo string `json:"clientNo"`
		}
		if len(raw) > 4096 || ddJSON(raw, &value) != nil || len(value.ClientNo) != 32 {
			return out, ddCCProtocol()
		}
		for _, c := range value.ClientNo {
			if !(c >= '0' && c <= '9' || c >= 'a' && c <= 'f') {
				return out, ddCCProtocol()
			}
		}
		out.clientNo = value.ClientNo
	} else if os.IsNotExist(err) {
		out.clientNo = ddRandom()[:32]
		raw, _ = json.Marshal(map[string]any{"clientNo": out.clientNo})
		if err = vault.ReplaceFile(ctx, path, raw); err != nil {
			return out, err
		}
	} else {
		return out, err
	}
	interfaces, err := net.Interfaces()
	if err != nil {
		return out, err
	}
	for _, iface := range interfaces {
		if len(iface.HardwareAddr) == 6 {
			out.mac = strings.ReplaceAll(iface.HardwareAddr.String(), ":", "-")
			break
		}
	}
	if out.mac == "" {
		return out, capability("publish.dd_device", "Windows network device identity is unavailable")
	}
	version := windows.RtlGetVersion()
	out.osVersion = fmt.Sprintf("%d.%d.%d", version.MajorVersion, version.MinorVersion, version.BuildNumber)
	return out, nil
}
