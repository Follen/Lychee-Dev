//go:build windows && amd64

package command

import (
	"bufio"
	"context"
	"encoding/json"
	"fmt"
	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/live"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/makiuchi-d/gozxing"
	"github.com/makiuchi-d/gozxing/qrcode"
	"golang.org/x/sys/windows"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	"syscall"
	"testing"
	"time"
	"unsafe"
)

type bindingBitmap struct {
	Size                   uint32
	Width, Height          int32
	Planes, Bits           uint16
	Compression, ImageSize uint32
	X, Y                   int32
	Used, Important        uint32
}
type bindingMessage struct {
	Window         uintptr
	Message        uint32
	WParam, LParam uintptr
	Time           uint32
	X, Y           int32
	Private        uint32
}

// This helper is copied as Wow.exe into a temporary synthetic installation.
// It owns one non-activating STATIC bitmap window, never any game window.
func TestNativeBindingWindowHelper(t *testing.T) {
	payload := os.Getenv("LYCHEEDEV_BINDING_FIXTURE")
	if payload == "" {
		t.Skip("isolated native helper")
	}
	bitmap, err := qrcode.NewQRCodeWriter().Encode(payload, gozxing.BarcodeFormat_QR_CODE, 600, 600, nil)
	if err != nil {
		t.Fatal(err)
	}
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()
	user := windows.NewLazySystemDLL("user32.dll")
	gdi := windows.NewLazySystemDLL("gdi32.dll")
	dpi := user.NewProc("SetThreadDpiAwarenessContext")
	previous, _, _ := dpi.Call(^uintptr(3))
	if previous != 0 {
		defer dpi.Call(previous)
	}
	header := bindingBitmap{Size: 40, Width: 600, Height: -600, Planes: 1, Bits: 32}
	var bits unsafe.Pointer
	hbitmap, _, err := gdi.NewProc("CreateDIBSection").Call(0, uintptr(unsafe.Pointer(&header)), 0, uintptr(unsafe.Pointer(&bits)), 0, 0)
	if hbitmap == 0 || bits == nil {
		t.Fatal("DIB creation", err)
	}
	defer gdi.NewProc("DeleteObject").Call(hbitmap)
	pixels := unsafe.Slice((*byte)(bits), 600*600*4)
	for y := 0; y < 600; y++ {
		for x := 0; x < 600; x++ {
			value := byte(255)
			if bitmap.Get(x, y) {
				value = 0
			}
			i := (y*600 + x) * 4
			pixels[i], pixels[i+1], pixels[i+2], pixels[i+3] = value, value, value, 0
		}
	}
	class, _ := windows.UTF16PtrFromString("STATIC")
	hwnd, _, err := user.NewProc("CreateWindowExW").Call(0x08000080, uintptr(unsafe.Pointer(class)), 0, 0x8000000e, 30, 30, 600, 600, 0, 0, 0, 0)
	if hwnd == 0 {
		t.Fatal("fixture window", err)
	}
	defer user.NewProc("DestroyWindow").Call(hwnd)
	// SS_BITMAP/STM_SETIMAGE; zero alpha avoids a control-owned bitmap copy.
	// https://learn.microsoft.com/en-us/windows/win32/controls/stm-setimage
	user.NewProc("SendMessageW").Call(hwnd, 0x172, 0, hbitmap)
	user.NewProc("ShowWindow").Call(hwnd, 4)
	user.NewProc("SetWindowPos").Call(hwnd, 1, 0, 0, 0, 0, 0x13)
	user.NewProc("UpdateWindow").Call(hwnd)
	thread := windows.GetCurrentThreadId()
	go func() {
		_, _ = io.Copy(io.Discard, os.Stdin)
		user.NewProc("PostThreadMessageW").Call(uintptr(thread), 0x12, 0, 0)
	}()
	fmt.Println("BINDING-FIXTURE ready")
	for {
		var message bindingMessage
		code, _, _ := user.NewProc("GetMessageW").Call(uintptr(unsafe.Pointer(&message)), 0, 0, 0)
		if int32(code) <= 0 {
			return
		}
		user.NewProc("TranslateMessage").Call(uintptr(unsafe.Pointer(&message)))
		user.NewProc("DispatchMessageW").Call(uintptr(unsafe.Pointer(&message)))
	}
}

func TestLiveBindNativeCLI(t *testing.T) {
	for _, region := range []string{"0,0,600,600", "window"} {
		t.Run(region, func(t *testing.T) { testLiveBindNativeRegion(t, region) })
	}
}

func testLiveBindNativeRegion(t *testing.T, region string) {
	if os.Getenv("LYCHEEDEV_TEST_DESKTOP") != "1" {
		t.Skip("requires an interactive desktop for native WGC")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 90*time.Second)
	defer cancel()
	root := t.TempDir()
	client := filepath.Join(root, "game", "_retail_")
	if err := os.MkdirAll(client, 0700); err != nil {
		t.Fatal(err)
	}
	for name, data := range map[string]string{".flavor.info": "Product Flavor!STRING:0\nwow\n", "version.txt": "12.1.0.69875\n"} {
		if err := os.WriteFile(filepath.Join(client, name), []byte(data), 0600); err != nil {
			t.Fatal(err)
		}
	}
	self, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	executable, err := os.ReadFile(self)
	if err != nil {
		t.Fatal(err)
	}
	helper := filepath.Join(client, "Wow.exe")
	if err := os.WriteFile(helper, executable, 0700); err != nil {
		t.Fatal(err)
	}
	expected := bridge.Signal{Schema: "lycheedev.signal.v1", Kind: "ready", Release: Version, SessionNonce: strings.Repeat("a", 32), Character: "BindingFixture", Realm: "FixtureRealm", GUID: "Player-1-123", Product: "retail", Build: "12.1.0.69875", Sequence: 1, InputReady: true}
	raw, _ := json.Marshal(expected)
	child := exec.CommandContext(ctx, helper, "-test.run=^TestNativeBindingWindowHelper$")
	child.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}
	child.Env = append(os.Environ(), "LYCHEEDEV_BINDING_FIXTURE="+string(raw))
	stdout, err := child.StdoutPipe()
	if err != nil {
		t.Fatal(err)
	}
	stdin, err := child.StdinPipe()
	if err != nil {
		t.Fatal(err)
	}
	if err := child.Start(); err != nil {
		t.Fatal(err)
	}
	defer func() { _ = stdin.Close(); _ = child.Process.Kill(); _ = child.Wait() }()
	ready := make(chan bool, 1)
	go func() {
		scanner := bufio.NewScanner(stdout)
		ready <- scanner.Scan() && scanner.Text() == "BINDING-FIXTURE ready"
	}()
	select {
	case ok := <-ready:
		if !ok {
			t.Fatal("helper did not create its window")
		}
	case <-ctx.Done():
		t.Fatal(ctx.Err())
	}
	cli := os.Getenv("LYCHEEDEV_BINDING_NODE")
	launcher := os.Getenv("LYCHEEDEV_BINDING_LAUNCHER")
	var prefix []string
	if cli != "" || launcher != "" {
		for _, path := range []string{cli, launcher} {
			info, err := os.Stat(path)
			if !filepath.IsAbs(path) || err != nil || !info.Mode().IsRegular() {
				t.Fatalf("installed launcher requires absolute regular files: %q (%v)", path, err)
			}
		}
		prefix = []string{launcher}
		t.Logf("using installed npm launcher: %s", launcher)
	} else {
		cli = filepath.Join(root, "lycheedev.exe")
		build := exec.CommandContext(ctx, "go", "build", "-o", cli, "./cmd/lycheedev")
		build.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}
		build.Dir, err = filepath.Abs(filepath.Join("..", ".."))
		if err != nil {
			t.Fatal(err)
		}
		if output, err := build.CombinedOutput(); err != nil {
			t.Fatalf("build: %v %s", err, output)
		}
	}
	home := filepath.Join(root, "home")
	invokeCLI := func(args ...string) Envelope {
		argv := append(append([]string{}, prefix...), args...)
		cmd := exec.CommandContext(ctx, cli, append(argv, "--home", home, "--format=json")...)
		cmd.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}
		output, err := cmd.CombinedOutput()
		if err != nil {
			t.Fatalf("CLI: %v %s", err, output)
		}
		var envelope Envelope
		if err := json.Unmarshal(output, &envelope); err != nil || !envelope.OK {
			t.Fatalf("envelope: %s %v", output, err)
		}
		return envelope
	}
	invokeCLI("init")
	spec := selection.SelectionSpec{Source: &selection.SourcePin{Repository: "fixture", Product: "retail", ExactCommit: strings.Repeat("a", 40), ParserRevision: "1"}}
	raw, _ = json.Marshal(spec)
	selectionFile := filepath.Join(root, "selection.json")
	if err := os.WriteFile(selectionFile, raw, 0600); err != nil {
		t.Fatal(err)
	}
	pinned := invokeCLI("target", "resolve", "--file", selectionFile)
	raw, _ = json.Marshal(pinned.Result)
	var pin selection.PinnedSet
	if err := json.Unmarshal(raw, &pin); err != nil {
		t.Fatal(err)
	}
	bound := invokeCLI("live", "bind", "--pid", strconv.Itoa(child.Process.Pid), "--snapshot", pin.ID, "--capture-area", region)
	raw, _ = json.Marshal(bound.Result)
	var session live.Connection
	if err := json.Unmarshal(raw, &session); err != nil {
		t.Fatal(err)
	}
	if session.Character != expected.Character || session.Realm != expected.Realm || session.Target.Window.ProcessID != uint32(child.Process.Pid) || session.Target.Window.ProcessStartedAt == 0 || session.ID == "" || len(bound.Warnings) != 1 {
		t.Fatalf("wrong native session: %+v", session)
	}
	retained, err := live.ReadWindowSession(ctx, home, session.ID)
	if err != nil || retained.Ready != expected {
		t.Fatalf("discovered receipt not preserved: %+v %v", retained.Ready, err)
	}
	verified := invokeCLI("evidence", "verify", session.CaptureID)
	if !verified.OK {
		t.Fatal("capture integrity failed")
	}
	history := invokeCLI("live", "session", session.ID)
	raw, _ = json.Marshal(history.Result)
	var saved live.Connection
	if err := json.Unmarshal(raw, &saved); err != nil || saved != session {
		t.Fatal("historical session differs", err)
	}
	probeFile := filepath.Join(root, "probe.lua")
	if err := os.WriteFile(probeFile, []byte("return 42"), 0600); err != nil {
		t.Fatal(err)
	}
	for _, mode := range []string{"missing", "ambiguous"} {
		if mode == "ambiguous" {
			for _, account := range []string{"Account-B", "Account-A"} {
				if err := os.MkdirAll(filepath.Join(client, "WTF", "Account", account, expected.Realm, expected.Character), 0700); err != nil {
					t.Fatal(err)
				}
			}
		}
		argv := append(append([]string{}, prefix...), "live", "run", "--session", session.ID, "--file", probeFile, "--home", home, "--format=json")
		command := exec.CommandContext(ctx, cli, argv...)
		command.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}
		output, err := command.CombinedOutput()
		exit, ok := err.(*exec.ExitError)
		if !ok || exit.ExitCode() != 2 {
			t.Fatalf("account choice %s: %v %s", mode, err, output)
		}
		var choice Envelope
		if err := json.Unmarshal(output, &choice); err != nil || choice.OK || choice.OperationID != "" || choice.Result != nil || choice.Error == nil || choice.Error.Code != "live.account_selection_required" || choice.Error.Stage != "selection" {
			t.Fatalf("choice envelope: %s %v", output, err)
		}
		accounts, ok := choice.Context["accounts"].([]any)
		if !ok || (mode == "missing" && len(accounts) != 0) || (mode == "ambiguous" && (len(accounts) != 2 || accounts[0] != "Account-A" || accounts[1] != "Account-B")) {
			t.Fatal("incorrect account choices", choice)
		}
	}
	t.Logf("native CLI binding verified pid=%d hwnd=%d session=%s capture=%s; synthetic window, not WoW", session.Target.Window.ProcessID, session.Target.Window.Handle, session.ID, session.CaptureID)
}
