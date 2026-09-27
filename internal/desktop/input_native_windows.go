//go:build windows && amd64

package desktop

// The receiver uses the same per-message native path proven by the isolated
// input lab. Production callers use postmessage only; sendinput is an explicit
// lab mode and is never selected as a fallback.
import (
	"context"
	"errors"
	"fmt"
	"time"
)

// The optional tagged lab installs this hook for its explicit foreground
// SendInput experiments. It is nil in the production binary.
var labSendPacket func(*nativeInput, uint16, uint16, uint32) error

type nativeInput struct {
	ctx     context.Context
	target  WindowIdentity
	mode    string
	guard   func(context.Context) error
	receipt InputReceipt
	down    []uint16
}

// withNativeInput reuses production identity checks and its cross-process mutex.
// guard is the caller's disk ownership check, also called before every message.
func withNativeInput(ctx context.Context, target WindowIdentity, mode string, guard func(context.Context) error, fn func(*nativeInput) error) (InputReceipt, error) {
	if guard == nil || fn == nil || (mode != "postmessage" && (mode != "sendinput" || labSendPacket == nil)) {
		return InputReceipt{}, errors.New("lab.invalid_options")
	}
	ctx, cancel := context.WithTimeout(ctx, 60*time.Second)
	defer cancel()
	return withInputLock(ctx, target, func() (receipt InputReceipt, err error) {
		in := &nativeInput{ctx: ctx, target: target, mode: mode, guard: guard}
		defer func() { err = errors.Join(err, in.release()); receipt = in.receipt }()
		if err = in.check(); err != nil {
			return receipt, err
		}
		err = fn(in)
		in.receipt.SubmissionComplete = err == nil
		return receipt, err
	})
}
func (in *nativeInput) check() error {
	if err := in.guard(in.ctx); err != nil {
		return err
	}
	if err := ConfirmWindow(in.ctx, in.target); err != nil {
		return err
	}
	minimized, _, _ := windowMinimized.Call(uintptr(in.target.Handle))
	if minimized != 0 {
		return errors.New("desktop.window_minimized")
	}
	if in.mode == "sendinput" {
		fg, _, _ := userLibrary.NewProc("GetForegroundWindow").Call()
		if fg != uintptr(in.target.Handle) {
			return errors.New("lab.foreground_required")
		}
	}
	return nil
}
func (in *nativeInput) Wait(d time.Duration) error {
	t := time.NewTimer(d)
	defer t.Stop()
	select {
	case <-t.C:
		return nil
	case <-in.ctx.Done():
		return in.ctx.Err()
	}
}
func (in *nativeInput) post(message uint32, value, bits uintptr) error {
	if err := in.check(); err != nil {
		return err
	}
	ok, _, err := postWindowMessage.Call(uintptr(in.target.Handle), uintptr(message), value, bits)
	if ok == 0 {
		return fmt.Errorf("lab.post: %w", err)
	}
	in.receipt.MessagesQueued++
	return nil
}

func (in *nativeInput) native(vk, scan uint16, flags uint32) error {
	if err := in.check(); err != nil {
		return err
	}
	return in.nativePacket(vk, scan, flags)
}
func (in *nativeInput) nativePacket(vk, scan uint16, flags uint32) error {
	if labSendPacket == nil {
		return errors.New("lab.sendinput_unavailable")
	}
	return labSendPacket(in, vk, scan, flags)
}
func (in *nativeInput) key(vk uint16, up, alt bool) error {
	if in.mode == "sendinput" {
		flags := uint32(0)
		if up {
			flags = 2
		}
		return in.native(vk, 0, flags)
	}
	scan, _, _ := mapVirtualKey.Call(uintptr(vk), 0)
	bits := uintptr(1) | (scan << 16)
	msg := uint32(0x100)
	if alt {
		msg = 0x104
		bits |= 1 << 29
	}
	if up {
		msg++
		bits |= 1<<30 | 1<<31
	}
	return in.post(msg, uintptr(vk), bits)
}
func (in *nativeInput) release() error {
	// Cleanup is bounded and only releases keys pressed by this attempt. Never
	// clear arbitrary modifiers, activate a window, or release into another HWND.
	cleanup, cancel := context.WithTimeout(context.WithoutCancel(in.ctx), time.Second)
	defer cancel()
	old := in.ctx
	in.ctx = cleanup
	defer func() { in.ctx = old }()
	var err error
	alt := false
	for _, vk := range in.down {
		if vk == 0x12 {
			alt = true
		}
	}
	for i := len(in.down) - 1; i >= 0; i-- {
		vk := in.down[i]
		// SendInput modifies the desktop key state. Even if focus moved, release our
		// injected keys globally, never leave a modifier held in the user's session.
		if in.mode == "sendinput" {
			err = errors.Join(err, in.nativePacket(vk, 0, 2))
		} else {
			// An ownership change stops new input, but must not strand our own
			// modifier key-down. Recheck identity and release only that key.
			if check := ConfirmWindow(cleanup, in.target); check != nil {
				err = errors.Join(err, check)
				continue
			}
			scan, _, _ := mapVirtualKey.Call(uintptr(vk), 0)
			bits := uintptr(1) | (scan << 16) | 1<<30 | 1<<31
			message := uintptr(0x101)
			if alt {
				message = 0x105
				bits |= 1 << 29
			}
			ok, _, sendErr := postWindowMessage.Call(uintptr(in.target.Handle), message, uintptr(vk), bits)
			if ok == 0 {
				err = errors.Join(err, fmt.Errorf("lab.release: %w", sendErr))
			} else {
				in.receipt.MessagesQueued++
			}
		}
		if vk == 0x12 {
			alt = false
		}
	}
	in.down = nil
	return err
}
func (in *nativeInput) Chord(action string) error {
	var keys []uint16
	switch action {
	case "wake":
		keys = []uint16{0x11, 0x12, 0xDD}
	case "submit":
		keys = []uint16{0x11, 0x12, 0x10, 0xDD}
	case "dismiss":
		keys = []uint16{0x11, 0x12, 0xDB}
	case "escape":
		keys = []uint16{0x1B}
	case "enter":
		keys = []uint16{0x0D}
	default:
		return errors.New("lab.unknown_action")
	}
	return in.chordKeys(keys)
}

func (in *nativeInput) ReceiverChord(chord string) error {
	key, err := receiverBindingKey(chord)
	if err != nil {
		return err
	}
	keys := []uint16{0x11, 0x12}
	if len(chord) >= len("ALT-CTRL-SHIFT-") && chord[:len("ALT-CTRL-SHIFT-")] == "ALT-CTRL-SHIFT-" {
		keys = append(keys, 0x10)
	}
	var vk uint16
	switch key {
	case "[":
		vk = 0xDB
	case "]":
		vk = 0xDD
	default:
		if len(key) == 1 {
			vk = uint16(key[0])
		} else {
			var number int
			_, _ = fmt.Sscanf(key, "F%d", &number)
			vk = uint16(0x70 + number - 1)
		}
	}
	return in.chordKeys(append(keys, vk))
}

func (in *nativeInput) chordKeys(keys []uint16) error {
	// The lab records the target layout; OEM brackets require live validation.
	for _, vk := range keys {
		pressed, _, _ := userLibrary.NewProc("GetAsyncKeyState").Call(uintptr(vk))
		if pressed&0x8000 != 0 {
			return errors.New("lab.key_already_held")
		}
	}
	alt := false
	for _, vk := range keys {
		if err := in.key(vk, false, alt || vk == 0x12); err != nil {
			return err
		}
		in.down = append(in.down, vk)
		if vk == 0x12 {
			alt = true
		}
		if err := in.Wait(40 * time.Millisecond); err != nil {
			return err
		}
	}
	for len(in.down) > 0 {
		vk := in.down[len(in.down)-1]
		if err := in.key(vk, true, alt); err != nil {
			return err
		}
		in.down = in.down[:len(in.down)-1]
		if vk == 0x12 {
			alt = false
		}
		if err := in.Wait(40 * time.Millisecond); err != nil {
			return err
		}
	}
	return nil
}
func (in *nativeInput) Text(text string) error {
	if len(text) == 0 || len(text) > 256 {
		return errors.New("lab.text_capacity")
	}
	for _, b := range []byte(text) {
		if b < 32 || b > 126 {
			return errors.New("lab.ascii_required")
		}
	}
	for _, b := range []byte(text) {
		var err error
		if in.mode == "sendinput" {
			err = in.native(0, uint16(b), 4)
			if err == nil {
				err = in.native(0, uint16(b), 6)
			}
		} else {
			err = in.post(0x102, uintptr(b), 1)
		}
		if err != nil {
			return err
		}
		if err = in.Wait(50 * time.Millisecond); err != nil {
			return err
		}
	}
	return nil
}
func (in *nativeInput) Reload() error {
	return in.resetAndEnter("/reload")
}

func (in *nativeInput) resetAndEnter(command string) error {
	for i := 0; i < 3; i++ {
		if err := in.Chord("escape"); err != nil {
			return err
		}
		if err := in.Wait(150 * time.Millisecond); err != nil {
			return err
		}
	}
	if err := in.Chord("enter"); err != nil {
		return err
	}
	if err := in.Wait(150 * time.Millisecond); err != nil {
		return err
	}
	if err := in.Text(command); err != nil {
		return err
	}
	return in.Chord("enter")
}
