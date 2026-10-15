//go:build windows && amd64

package desktop

import (
	"errors"
	"fmt"
	"runtime"
	"sync"
	"time"
	"unsafe"

	"golang.org/x/sys/windows"
)

var (
	setKeyboardHook     = userLibrary.NewProc("SetWindowsHookExW")
	unhookKeyboard      = userLibrary.NewProc("UnhookWindowsHookEx")
	nextKeyboardHook    = userLibrary.NewProc("CallNextHookEx")
	keyboardGetMessage  = userLibrary.NewProc("GetMessageW")
	keyboardPeekMessage = userLibrary.NewProc("PeekMessageW")
	keyboardPostThread  = userLibrary.NewProc("PostThreadMessageW")
	keyboardKeyState    = userLibrary.NewProc("GetAsyncKeyState")
	keyboardModule      = windows.NewLazySystemDLL("kernel32.dll").NewProc("GetModuleHandleW")
	keyboardListeners   sync.Map // installer thread ID -> *KeyboardActivity
	// NewCallback thunks cannot be freed. Reuse one across monitor lifetimes.
	keyboardHookThunk = windows.NewCallback(func(code, message, data uintptr) uintptr {
		if int32(code) >= 0 && (message == 0x100 || message == 0x101 || message == 0x104 || message == 0x105) {
			if listener, ok := keyboardListeners.Load(windows.GetCurrentThreadId()); ok {
				listener.(*KeyboardActivity).state.note(time.Now())
			}
		}
		// Observe only: every event continues through the normal hook chain.
		result, _, _ := nextKeyboardHook.Call(0, code, message, data)
		return result
	})
)

type keyboardMessage struct {
	window         uintptr
	message        uint32
	wParam, lParam uintptr
	time           uint32
	x, y           int32
	private        uint32
}

// KeyboardActivity observes keyboard events until Close. It must span retry
// attempts so a busy keyboard does not restart the quiet period on each retry.
type KeyboardActivity struct {
	state     keyboardActivityState
	threadID  uint32
	done      chan struct{}
	loopErr   error
	closeOnce sync.Once
	closeErr  error
}

func NewKeyboardActivity() (*KeyboardActivity, error) {
	m := &KeyboardActivity{done: make(chan struct{})}
	ready := make(chan error, 1)
	go m.run(ready)
	if err := <-ready; err != nil {
		<-m.done
		return nil, err
	}
	return m, nil
}

func (m *KeyboardActivity) run(ready chan<- error) {
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()
	defer close(m.done)
	defer m.state.stop(errors.New("desktop.keyboard_monitor_closed"))
	m.threadID = windows.GetCurrentThreadId()
	// Create the thread queue before publishing readiness, including when no
	// keyboard event has arrived yet. Close can then reliably post WM_QUIT.
	var message keyboardMessage
	keyboardPeekMessage.Call(uintptr(unsafe.Pointer(&message)), 0, 0, 0, 0)
	module, _, err := keyboardModule.Call(0)
	if module == 0 {
		ready <- fmt.Errorf("desktop.keyboard_module: %w", err)
		return
	}
	keyboardListeners.Store(m.threadID, m)
	defer keyboardListeners.Delete(m.threadID)
	hook, _, err := setKeyboardHook.Call(13, keyboardHookThunk, module, 0) // WH_KEYBOARD_LL
	if hook == 0 {
		ready <- fmt.Errorf("desktop.keyboard_hook: %w", err)
		return
	}
	defer func() {
		if ok, _, err := unhookKeyboard.Call(hook); ok == 0 {
			m.loopErr = errors.Join(m.loopErr, fmt.Errorf("desktop.keyboard_unhook: %w", err))
		}
	}()
	// Events before installation are unknown, so first use also waits 500ms.
	m.state.note(time.Now())
	ready <- nil
	for {
		result, _, err := keyboardGetMessage.Call(uintptr(unsafe.Pointer(&message)), 0, 0, 0)
		if int32(result) <= 0 {
			if int32(result) == -1 {
				m.loopErr = fmt.Errorf("desktop.keyboard_messages: %w", err)
			}
			return
		}
	}
}

// Check never waits. The caller releases its leases and obtains fresh game
// INPUT on the next attempt. GetAsyncKeyState is read outside the hook callback,
// since Windows calls that callback before updating asynchronous key state.
func (m *KeyboardActivity) Check() error {
	return m.state.check(time.Now(), func(vk uint16) bool {
		pressed, _, _ := keyboardKeyState.Call(uintptr(vk))
		return pressed&0x8000 != 0
	})
}

func (m *KeyboardActivity) Close() error {
	m.closeOnce.Do(func() {
		select {
		case <-m.done:
			m.closeErr = m.loopErr
			return
		default:
		}
		if ok, _, err := keyboardPostThread.Call(uintptr(m.threadID), 0x12, 0, 0); ok == 0 { // WM_QUIT
			m.closeErr = fmt.Errorf("desktop.keyboard_stop: %w", err)
			return
		}
		<-m.done
		m.closeErr = m.loopErr
	})
	return m.closeErr
}
