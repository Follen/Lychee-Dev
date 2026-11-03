//go:build windows && amd64

package publish

import (
	"context"
	"crypto/subtle"
	"encoding/json"
	"errors"
	"github.com/Microsoft/go-winio"
	"golang.org/x/sys/windows"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"syscall"
	"time"
	"unsafe"
)

const ddChannelFlag = "--publish-dd-channel-helper"
const ddChannelWire = "lycheedev.publish.dd-channel.v1"

type ddChannelAnchor struct {
	GUIPID, NetworkPID                  int
	GUICreated, NetworkCreated          uint64
	Installation, NetworkImage, Profile string
}
type ddChannelRequest struct {
	Schema, Nonce string
	Deadline      int64
	Anchor        ddChannelAnchor
}

func (d *ddDesktop) channelsWithElevation(ctx context.Context) (any, error) {
	value, err := d.channels(ctx)
	var issue *Error
	if errors.As(err, &issue) && issue.Code == "publish.dd_channel_memory_denied" {
		return ddChannelElevated(ctx, d)
	}
	return value, err
}
func ddSameUser(pid uint32) error {
	process, err := windows.OpenProcess(windows.PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
	if err != nil {
		return ddManagedError()
	}
	defer windows.CloseHandle(process)
	var token windows.Token
	if windows.OpenProcessToken(process, windows.TOKEN_QUERY, &token) != nil {
		return ddManagedError()
	}
	defer token.Close()
	other, err := token.GetTokenUser()
	if err != nil {
		return ddManagedError()
	}
	current, err := windows.GetCurrentProcessToken().GetTokenUser()
	if err != nil || !windows.EqualSid(other.User.Sid, current.User.Sid) {
		return capability("publish.dd_channel_user", "DD channel helper requires the original Windows user")
	}
	return nil
}
func ddElevate(executable string, args []string) (windows.Handle, error) {
	// SHELLEXECUTEINFOW, Windows amd64. The original returned process handle is
	// retained for cleanup; neither PID discovery nor a second launch is used.
	type shellInfo struct {
		Size, Mask                        uint32
		Window                            windows.Handle
		Verb, File, Parameters, Directory *uint16
		Show                              int32
		Instance                          windows.Handle
		IDList                            uintptr
		Class                             *uint16
		ClassKey                          windows.Handle
		HotKey                            uint32
		Icon, Process                     windows.Handle
	}
	joined := make([]string, len(args))
	for i, arg := range args {
		joined[i] = syscall.EscapeArg(arg)
	}
	verb, _ := windows.UTF16PtrFromString("runas")
	file, err := windows.UTF16PtrFromString(executable)
	if err != nil {
		return 0, err
	}
	parameters, err := windows.UTF16PtrFromString(strings.Join(joined, " "))
	if err != nil {
		return 0, err
	}
	directory, _ := windows.UTF16PtrFromString(filepath.Dir(executable))
	info := shellInfo{Size: uint32(unsafe.Sizeof(shellInfo{})), Mask: 0x40 | 0x100 | 0x400, Verb: verb, File: file, Parameters: parameters, Directory: directory, Show: 0}
	proc := windows.NewLazySystemDLL("shell32.dll").NewProc("ShellExecuteExW")
	ok, _, e := proc.Call(uintptr(unsafe.Pointer(&info)))
	if ok == 0 {
		if e == windows.ERROR_CANCELLED {
			return 0, &Error{Code: "publish.dd_channel_cancelled", Message: "DD channel permission request was cancelled", Exit: 7}
		}
		return 0, capability("publish.dd_channel_helper", "the one-shot DD channel helper could not start")
	}
	if info.Process == 0 {
		return 0, ddManagedError()
	}
	return info.Process, nil
}
func ddChannelElevated(ctx context.Context, d *ddDesktop) (any, error) {
	ctx, cancel := context.WithTimeout(ctx, 55*time.Second)
	defer cancel()
	if err := d.verify(ctx); err != nil {
		return nil, err
	}
	exe, err := os.Executable()
	if err != nil {
		return nil, err
	}
	pin, err := openArtifact(exe)
	if err != nil {
		return nil, err
	}
	defer pin.Close()
	created, err := ddProcessCreated(os.Getpid())
	if err != nil {
		return nil, err
	}
	nonce := ddRandom()
	pipe := `\\.\pipe\lycheedev-dd-channels-` + nonce
	sddl, err := ddUserSDDL()
	if err != nil {
		return nil, err
	}
	listener, err := winio.ListenPipe(pipe, &winio.PipeConfig{SecurityDescriptor: sddl})
	if err != nil {
		return nil, err
	}
	defer listener.Close()
	stop := context.AfterFunc(ctx, func() { listener.Close() })
	defer stop()
	deadline, _ := ctx.Deadline()
	handle, err := ddElevate(exe, []string{ddChannelFlag, pipe, nonce, strconv.Itoa(os.Getpid()), strconv.FormatUint(created, 10), strconv.FormatInt(deadline.UnixMilli(), 10)})
	if err != nil {
		return nil, err
	}
	defer windows.CloseHandle(handle)
	defer func() {
		state, _ := windows.WaitForSingleObject(handle, 0)
		if state == uint32(windows.WAIT_TIMEOUT) {
			windows.TerminateProcess(handle, 7)
			windows.WaitForSingleObject(handle, 5000)
		}
	}()
	if err = ctx.Err(); err != nil {
		return nil, err
	}
	pid, err := windows.GetProcessId(handle)
	if err != nil {
		return nil, err
	}
	if err = ddSameUser(pid); err != nil {
		return nil, err
	}
	helperImage, _, err := ddProcessImage(pid, false)
	if err != nil || !strings.EqualFold(filepath.Clean(helperImage), filepath.Clean(exe)) {
		return nil, ddManagedError()
	}
	conn, err := listener.Accept()
	if err != nil {
		return nil, ddManagedError()
	}
	defer conn.Close()
	conn.SetDeadline(deadline)
	fd, ok := conn.(interface{ Fd() uintptr })
	var peer uint32
	if !ok || windows.GetNamedPipeClientProcessId(windows.Handle(fd.Fd()), &peer) != nil || peer != pid {
		return nil, ddManagedError()
	}
	var hello ddSessionReply
	if ddReadFrameLimit(conn, &hello, 1<<20) != nil || hello.Schema != ddChannelWire || subtle.ConstantTimeCompare([]byte(hello.ID), []byte(nonce)) != 1 {
		return nil, ddManagedError()
	}
	request := ddChannelRequest{Schema: ddChannelWire, Nonce: nonce, Deadline: deadline.UnixMilli(), Anchor: ddChannelAnchor{d.guiPID, d.networkPID, d.guiCreated, d.networkCreated, d.installation, d.networkImage, d.profile}}
	if ddWriteFrame(conn, request) != nil {
		return nil, ddManagedError()
	}
	var reply ddSessionReply
	if ddReadFrameLimit(conn, &reply, 1<<20) != nil || reply.Schema != ddChannelWire || subtle.ConstantTimeCompare([]byte(reply.ID), []byte(nonce)) != 1 {
		return nil, ddManagedError()
	}
	if reply.Error != nil {
		reply.Error.Exit = reply.Exit
		return nil, reply.Error
	}
	if err = ddValidateChannelChoices(reply.Value); err != nil {
		return nil, err
	}
	if err = d.verify(ctx); err != nil {
		return nil, err
	}
	conn.Close()
	remaining := time.Until(deadline)
	if remaining <= 0 {
		return nil, context.DeadlineExceeded
	}
	wait, err := windows.WaitForSingleObject(handle, uint32(min(remaining.Milliseconds(), 5000)))
	if err != nil || wait != windows.WAIT_OBJECT_0 {
		return nil, capability("publish.dd_channel_cleanup", "DD channel helper did not finish cleanup")
	}
	var code uint32
	if windows.GetExitCodeProcess(handle, &code) != nil || code != 0 {
		return nil, capability("publish.dd_channel_cleanup", "DD channel helper cleanup failed")
	}
	return reply.Value, nil
}
func ddValidateChannelChoices(value any) error {
	raw, err := json.Marshal(value)
	if err != nil || len(raw) > 1<<20 {
		return ddManagedError()
	}
	m, ok := value.(map[string]any)
	if !ok || len(m) != 3 {
		return ddManagedError()
	}
	dependencies, ok := m["dependencies"].([]any)
	if !ok || len(dependencies) != 1 {
		return ddManagedError()
	}
	dependency, ok := dependencies[0].(map[string]any)
	if !ok || len(dependency) != 2 || dependency["parent"] != "room_id" {
		return ddManagedError()
	}
	children, ok := dependency["children"].([]any)
	if !ok || len(children) != 2 || children[0] != "channel_id" || children[1] != "channel_type" {
		return ddManagedError()
	}
	items, ok := m["items"].([]any)
	if !ok || len(items) > 10000 || ddInteger(m["total"]) != int64(len(items)) {
		return ddManagedError()
	}
	for _, item := range items {
		row, ok := item.(map[string]any)
		if !ok || len(row) != 5 {
			return ddManagedError()
		}
		for _, key := range []string{"room_id", "room_name", "channel_id", "channel_name", "channel_type"} {
			text, ok := row[key].(string)
			if !ok || len(text) > 4096 || key == "room_id" && text == "" {
				return ddManagedError()
			}
		}
	}
	return nil
}
func ddHandleChannelHelper(ctx context.Context, args []string) (bool, int) {
	if len(args) == 0 || args[0] != ddChannelFlag {
		return false, 0
	}
	if len(args) != 6 {
		return true, 2
	}
	err := ddRunChannelHelper(ctx, args)
	if err != nil {
		return true, ddSessionError(err).Exit
	}
	return true, 0
}
func ddRunChannelHelper(ctx context.Context, args []string) error {
	nonce := args[2]
	if len(nonce) != 48 || args[1] != `\\.\pipe\lycheedev-dd-channels-`+nonce {
		return ddManagedError()
	}
	pid, err := strconv.ParseUint(args[3], 10, 32)
	if err != nil {
		return ddManagedError()
	}
	created, err := strconv.ParseUint(args[4], 10, 64)
	if err != nil {
		return ddManagedError()
	}
	ms, err := strconv.ParseInt(args[5], 10, 64)
	if err != nil || ms <= time.Now().UnixMilli() || ms > time.Now().Add(56*time.Second).UnixMilli() {
		return ddManagedError()
	}
	ctx, cancel := context.WithDeadline(ctx, time.UnixMilli(ms))
	defer cancel()
	watch := time.AfterFunc(time.Until(time.UnixMilli(ms))+time.Second, func() { os.Exit(5) })
	defer watch.Stop()
	verifyParent := func(context.Context) error {
		actual, e := ddProcessCreated(int(pid))
		if e != nil || actual != created {
			return ddManagedError()
		}
		if e = ddSameUser(uint32(pid)); e != nil {
			return e
		}
		parentImage, _, e := ddProcessImage(uint32(pid), false)
		if e != nil {
			return e
		}
		exe, e := os.Executable()
		if e != nil || !strings.EqualFold(filepath.Clean(parentImage), filepath.Clean(exe)) {
			return ddManagedError()
		}
		return nil
	}
	if err = verifyParent(ctx); err != nil {
		return err
	}
	conn, err := winio.DialPipeContext(ctx, args[1])
	if err != nil {
		return ddManagedError()
	}
	defer conn.Close()
	conn.SetDeadline(time.UnixMilli(ms))
	stop := context.AfterFunc(ctx, func() { conn.Close() })
	defer stop()
	fd, ok := conn.(interface{ Fd() uintptr })
	var server uint32
	if !ok || windows.GetNamedPipeServerProcessId(windows.Handle(fd.Fd()), &server) != nil || server != uint32(pid) {
		return ddManagedError()
	}
	if ddWriteFrame(conn, ddSessionReply{Schema: ddChannelWire, ID: nonce}) != nil {
		return ddManagedError()
	}
	var request ddChannelRequest
	if ddReadFrameLimit(conn, &request, 1<<20) != nil || request.Schema != ddChannelWire || request.Nonce != nonce || request.Deadline != ms {
		return ddManagedError()
	}
	a := request.Anchor
	if !filepath.IsAbs(a.Installation) || !filepath.IsAbs(a.Profile) || !filepath.IsAbs(a.NetworkImage) {
		return ddManagedError()
	}
	d := &ddDesktop{guiPID: a.GUIPID, networkPID: a.NetworkPID, guiCreated: a.GUICreated, networkCreated: a.NetworkCreated, installation: a.Installation, networkImage: a.NetworkImage, profile: a.Profile}
	current, err := ddSelectDesktop(ctx, a.Installation)
	if err != nil || current == nil || *current != *d {
		return ddManagedError()
	}
	value, err := d.channels(ctx)
	if err == nil {
		err = verifyParent(ctx)
	}
	reply := ddSessionReply{Schema: ddChannelWire, ID: nonce, Value: value}
	if err != nil {
		reply.Value = nil
		reply.Error = ddSessionError(err)
		reply.Exit = reply.Error.Exit
	}
	if ddWriteFrame(conn, reply) != nil {
		return ddManagedError()
	}
	return err
}
