//go:build windows && amd64

package desktop

import (
	"context"
	"fmt"
	"image"
	"os"
	"runtime"
	"sync"
	"testing"
	"time"
	"unicode/utf16"
	"unsafe"

	"github.com/makiuchi-d/gozxing"
	"github.com/makiuchi-d/gozxing/qrcode"
	"golang.org/x/sys/windows"
)

type testWindowClass struct {
	Style                              uint32
	Procedure                          uintptr
	ClassExtra, WindowExtra            int32
	Instance, Icon, Cursor, Background uintptr
	Menu, Name                         *uint16
}
type testMessage struct {
	Window         uintptr
	Message        uint32
	WParam, LParam uintptr
	Time           uint32
	X, Y           int32
	Private        uint32
}
type testPaint struct {
	DC                       uintptr
	Erase                    int32
	Left, Top, Right, Bottom int32
	Restore, Update          int32
	Reserved                 [32]byte
}
type testBitmapHeader struct {
	Size                   uint32
	Width, Height          int32
	Planes, Bits           uint16
	Compression, ImageSize uint32
	X, Y                   int32
	Used, Important        uint32
}
type inputPacket struct {
	message     uint32
	value, bits uintptr
}
type fixtureWindow struct {
	identity      WindowIdentity
	done          chan struct{}
	packets       chan inputPacket
	pixels        []byte
	width, height int
	once          sync.Once
}

var (
	fixtureWindows    sync.Map
	fixtureGDI        = windows.NewLazySystemDLL("gdi32.dll")
	fixtureDefault    = userLibrary.NewProc("DefWindowProcW")
	fixtureBeginPaint = userLibrary.NewProc("BeginPaint")
	fixtureEndPaint   = userLibrary.NewProc("EndPaint")
	fixtureDraw       = fixtureGDI.NewProc("SetDIBitsToDevice")
	fixtureQuit       = userLibrary.NewProc("PostQuitMessage")
	fixtureProcedure  = windows.NewCallback(func(hwnd uintptr, message uint32, wparam, lparam uintptr) uintptr {
		if message == 2 {
			fixtureQuit.Call(0)
			return 0
		}
		if value, ok := fixtureWindows.Load(hwnd); ok {
			window := value.(*fixtureWindow)
			switch message {
			case 0x100, 0x101, 0x102:
				select {
				case window.packets <- inputPacket{message, wparam, lparam}:
				default:
				}
				return 0
			case 0x0f:
				var paint testPaint
				dc, _, _ := fixtureBeginPaint.Call(hwnd, uintptr(unsafe.Pointer(&paint)))
				header := testBitmapHeader{Size: 40, Width: int32(window.width), Height: -int32(window.height), Planes: 1, Bits: 32}
				fixtureDraw.Call(dc, 0, 0, uintptr(window.width), uintptr(window.height), 0, 0, 0, uintptr(window.height), uintptr(unsafe.Pointer(&window.pixels[0])), uintptr(unsafe.Pointer(&header)), 0)
				fixtureEndPaint.Call(hwnd, uintptr(unsafe.Pointer(&paint)))
				return 0
			}
		}
		result, _, _ := fixtureDefault.Call(hwnd, uintptr(message), wparam, lparam)
		return result
	})
)

func openFixtureWindow(t *testing.T, picture image.Image) *fixtureWindow {
	t.Helper()
	f := &fixtureWindow{done: make(chan struct{}), packets: make(chan inputPacket, 512), width: picture.Bounds().Dx(), height: picture.Bounds().Dy()}
	f.pixels = make([]byte, f.width*f.height*4)
	for y := 0; y < f.height; y++ {
		for x := 0; x < f.width; x++ {
			r, g, b, _ := picture.At(x, y).RGBA()
			offset := (y*f.width + x) * 4
			f.pixels[offset] = byte(b >> 8)
			f.pixels[offset+1] = byte(g >> 8)
			f.pixels[offset+2] = byte(r >> 8)
			f.pixels[offset+3] = 255
		}
	}
	ready := make(chan error, 1)
	go func() {
		defer close(f.done)
		runtime.LockOSThread()
		defer runtime.UnlockOSThread()
		dpiProc := userLibrary.NewProc("SetThreadDpiAwarenessContext")
		previous, _, _ := dpiProc.Call(^uintptr(3))
		if previous != 0 {
			defer dpiProc.Call(previous)
		}
		name, _ := windows.UTF16PtrFromString(fmt.Sprintf("LycheeToolkitFixture-%d", time.Now().UnixNano()))
		module, _, _ := windows.NewLazySystemDLL("kernel32.dll").NewProc("GetModuleHandleW").Call(0)
		class := testWindowClass{Procedure: fixtureProcedure, Instance: module, Name: name}
		atom, _, err := userLibrary.NewProc("RegisterClassW").Call(uintptr(unsafe.Pointer(&class)))
		if atom == 0 {
			ready <- err
			return
		}
		defer userLibrary.NewProc("UnregisterClassW").Call(uintptr(unsafe.Pointer(name)), module)
		hwnd, _, err := userLibrary.NewProc("CreateWindowExW").Call(0x08000080, uintptr(unsafe.Pointer(name)), uintptr(unsafe.Pointer(name)), 0x80000000, 30, 30, uintptr(f.width), uintptr(f.height), 0, 0, module, 0)
		if hwnd == 0 {
			ready <- err
			return
		}
		fixtureWindows.Store(hwnd, f)
		defer fixtureWindows.Delete(hwnd)
		f.identity, err = inspectWindow(hwnd)
		if err != nil {
			userLibrary.NewProc("DestroyWindow").Call(hwnd)
			ready <- err
			return
		}
		// Show behind other windows and never activate; WGC must capture even
		// when the user's foreground application occludes this test surface.
		userLibrary.NewProc("ShowWindow").Call(hwnd, 4)
		userLibrary.NewProc("SetWindowPos").Call(hwnd, 1, 0, 0, 0, 0, 0x13)
		userLibrary.NewProc("UpdateWindow").Call(hwnd)
		ready <- nil
		get := userLibrary.NewProc("GetMessageW")
		translate := userLibrary.NewProc("TranslateMessage")
		dispatch := userLibrary.NewProc("DispatchMessageW")
		for {
			var message testMessage
			result, _, _ := get.Call(uintptr(unsafe.Pointer(&message)), 0, 0, 0)
			if int32(result) <= 0 {
				return
			}
			translate.Call(uintptr(unsafe.Pointer(&message)))
			dispatch.Call(uintptr(unsafe.Pointer(&message)))
		}
	}()
	select {
	case err := <-ready:
		if err != nil {
			t.Fatal(err)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("fixture creation timeout")
	}
	t.Cleanup(func() {
		f.once.Do(func() { postWindowMessage.Call(uintptr(f.identity.Handle), 0x10, 0, 0) })
		select {
		case <-f.done:
		case <-time.After(5 * time.Second):
			t.Error("fixture did not close")
		}
	})
	return f
}

func TestBackgroundInputOwnWindow(t *testing.T) {
	f := openFixtureWindow(t, image.NewNRGBA(image.Rect(0, 0, 120, 80)))
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	const command = "/dev 中文 😀"
	receipt, err := QueueCommand(ctx, f.identity, command)
	if err != nil {
		t.Fatal(err)
	}
	if !receipt.SubmissionComplete {
		t.Fatal(receipt)
	}
	var chars []uint16
	downs, ups := 0, 0
	for i := 0; i < receipt.MessagesQueued; i++ {
		select {
		case packet := <-f.packets:
			switch packet.message {
			case 0x100:
				downs++
				if packet.value != 13 || packet.bits&0xffff != 1 || packet.bits&(1<<31) != 0 {
					t.Fatalf("keydown: %+v", packet)
				}
			case 0x101:
				ups++
				if packet.bits&(1<<30|1<<31) != (1<<30 | 1<<31) {
					t.Fatalf("keyup: %+v", packet)
				}
			case 0x102:
				if packet.value != 13 {
					chars = append(chars, uint16(packet.value))
				} else {
					i--
				}
			}
		case <-ctx.Done():
			t.Fatal(ctx.Err())
		}
	}
	if string(utf16.Decode(chars)) != command || downs != 2 || ups != 2 {
		t.Fatalf("decoded=%q down=%d up=%d", string(utf16.Decode(chars)), downs, ups)
	}
}

func TestWGCQRCodeOwnWindow(t *testing.T) {
	if os.Getenv("LYCHEEDEV_TEST_DESKTOP") != "1" {
		t.Skip("requires an interactive desktop for native WGC")
	}
	const payload = `{"schema":"fixture","text":"WGC 中文"}`
	bitmap, err := qrcode.NewQRCodeWriter().Encode(payload, gozxing.BarcodeFormat_QR_CODE, 600, 600, map[gozxing.EncodeHintType]interface{}{gozxing.EncodeHintType_CHARACTER_SET: "UTF-8"})
	if err != nil {
		t.Fatal(err)
	}
	f := openFixtureWindow(t, bitmap)
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	stream, err := CaptureFrames(ctx, f.identity, image.Rect(0, 0, 600, 600))
	if err != nil {
		t.Fatal(err)
	}
	defer stream.Close()
	for {
		frame, err := stream.Next(ctx)
		if err != nil {
			t.Fatal(err)
		}
		texts, err := DecodeSymbols(frame)
		if err != nil {
			t.Fatal(err)
		}
		for _, text := range texts {
			// DecodeSymbols returns the transmitted bytes re-spelled as
			// ISO-8859-1 text; BytesFromSymbolText is the exact payload.
			if string(BytesFromSymbolText(text)) == payload {
				t.Log("native WGC ROI decoded exact UTF-8 fixture bytes")
				return
			}
		}
	}
}
