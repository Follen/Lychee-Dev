//go:build windows

package luals

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	winio "github.com/Microsoft/go-winio"
	"golang.org/x/sys/windows"
	"io"
	"net"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"unsafe"
)

const BrokerWire = "lycheedev.luals-broker.v1"

// BrokerScope is tied to the current logon SID, not merely the account SID.
// Another logon of the same account cannot connect. winio's first FILE_CREATE
// requires an absent endpoint and every instance rejects remote clients.
func BrokerScope(home string) (endpoint, sddl, scope string, err error) {
	home, err = filepath.EvalSymlinks(home)
	if err != nil {
		return
	}
	home, err = filepath.Abs(home)
	if err != nil {
		return
	}
	token := windows.GetCurrentProcessToken()
	groups, e := token.GetTokenGroups()
	if e != nil {
		err = e
		return
	}
	sid := ""
	for _, group := range groups.AllGroups() {
		if group.Attributes&windows.SE_GROUP_LOGON_ID == windows.SE_GROUP_LOGON_ID {
			sid = group.Sid.String()
			break
		}
	}
	if sid == "" {
		err = fmt.Errorf("%w: logon SID unavailable", ErrUnavailable)
		return
	}
	digest := sha256.Sum256([]byte(strings.ToLower(home) + "\x00" + sid))
	endpoint = `\\.\pipe\lycheedev-source-` + hex.EncodeToString(digest[:16])
	sddl = "D:P(A;;GA;;;" + sid + ")"
	login := sha256.Sum256([]byte(sid))
	scope = filepath.Join(os.TempDir(), "lycheedev-source-broker", hex.EncodeToString(login[:16]))
	return
}
func ListenBrokerPipe(endpoint, sddl string) (net.Listener, error) {
	return winio.ListenPipe(endpoint, &winio.PipeConfig{SecurityDescriptor: sddl, InputBufferSize: 64 << 10, OutputBufferSize: 64 << 10})
}
func DialBrokerPipe(ctx context.Context, endpoint string) (net.Conn, error) {
	return winio.DialPipeContext(ctx, endpoint)
}

// VerifyBrokerPeer binds a connection to an actual server process, its creation
// identity and the exact executable bytes of this caller. A stale PID or pipe
// name never authorizes killing or taking over any process.
func VerifyBrokerPeer(conn net.Conn, pid uint32, started uint64, executableHash string) error {
	file, ok := conn.(interface{ Fd() uintptr })
	if !ok {
		return ErrUnavailable
	}
	var actual uint32
	if err := windows.GetNamedPipeServerProcessId(windows.Handle(file.Fd()), &actual); err != nil || actual != pid {
		return fmt.Errorf("%w: broker pipe process mismatch", ErrUnavailable)
	}
	identity, hash, err := BrokerProcessIdentity(actual)
	if err != nil || identity != started || hash != executableHash {
		return fmt.Errorf("%w: broker executable/creation mismatch", ErrUnavailable)
	}
	return nil
}
func BrokerProcessIdentity(pid uint32) (uint64, string, error) {
	process, err := windows.OpenProcess(windows.PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
	if err != nil {
		return 0, "", err
	}
	defer windows.CloseHandle(process)
	var created, exit, kernel, user windows.Filetime
	if err = windows.GetProcessTimes(process, &created, &exit, &kernel, &user); err != nil {
		return 0, "", err
	}
	buf := make([]uint16, 32768)
	n := uint32(len(buf))
	if err = windows.QueryFullProcessImageName(process, 0, &buf[0], &n); err != nil {
		return 0, "", err
	}
	file, err := os.Open(windows.UTF16ToString(buf[:n]))
	if err != nil {
		return 0, "", err
	}
	defer file.Close()
	h := sha256.New()
	copied, copyErr := io.Copy(h, io.LimitReader(file, (128<<20)+1))
	if copyErr != nil || copied > 128<<20 {
		err = copyErr
		if err == nil {
			err = ErrBudget
		}
		return 0, "", err
	}
	return uint64(created.HighDateTime)<<32 | uint64(created.LowDateTime), hex.EncodeToString(h.Sum(nil)), nil
}

// StartBroker only launches this CLI executable. The child proves the launching
// process's exact executable and creation identity before accepting its fixed
// home/release arguments. No runtime executable or shell text crosses IPC.
func StartBroker(commandPath string, args []string) error {
	exe, err := os.Executable()
	if err != nil {
		return err
	}
	exe, err = filepath.Abs(exe)
	if err != nil {
		return err
	}
	if commandPath != exe {
		return ErrUnavailable
	}
	command := newBrokerCommand(exe, args)
	command.Env = append(os.Environ(), "LYCHEEDEV_BROKER_START="+args[len(args)-1])
	if err = command.Start(); err != nil {
		return err
	}
	return command.Process.Release()
}

// BrokerJobBudget applies only to the opt-in worker. The aggregate owner admits
// one job per logon; this is a hard ceiling, not a claimed measured RSS target.
func setBrokerJobMemory(job windows.Handle) error {
	limit := windows.JOBOBJECT_EXTENDED_LIMIT_INFORMATION{}
	limit.BasicLimitInformation.LimitFlags = windows.JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | windows.JOB_OBJECT_LIMIT_JOB_MEMORY
	limit.JobMemoryLimit = 2 << 30
	_, err := windows.SetInformationJobObject(job, windows.JobObjectExtendedLimitInformation, uintptr(unsafe.Pointer(&limit)), uint32(unsafe.Sizeof(limit)))
	return err
}

// OwnBrokerJob includes the Go broker, Git admission helpers and LuaLS subtree
// in one aggregate 2 GiB commit ceiling. On normal shutdown composition joins
// all workers first, then disarms kill-on-close for the exiting owner itself.
func OwnBrokerJob() (func(), error) {
	job, err := windows.CreateJobObject(nil, nil)
	if err != nil {
		return nil, err
	}
	if err = setBrokerJobMemory(job); err != nil {
		windows.CloseHandle(job)
		return nil, err
	}
	if err = windows.AssignProcessToJobObject(job, windows.CurrentProcess()); err != nil {
		windows.CloseHandle(job)
		return nil, err
	}
	var once sync.Once
	return func() {
		once.Do(func() {
			limit := windows.JOBOBJECT_EXTENDED_LIMIT_INFORMATION{}
			limit.BasicLimitInformation.LimitFlags = windows.JOB_OBJECT_LIMIT_JOB_MEMORY
			limit.JobMemoryLimit = 2 << 30
			_, _ = windows.SetInformationJobObject(job, windows.JobObjectExtendedLimitInformation, uintptr(unsafe.Pointer(&limit)), uint32(unsafe.Sizeof(limit)))
			windows.CloseHandle(job)
		})
	}, nil
}
