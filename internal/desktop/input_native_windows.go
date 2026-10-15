//go:build windows && amd64

package desktop

// Native mailbox input sends only the observed wake/reload chords.
import (
	"context"
	"errors"
	"fmt"
	"time"
)

type nativeInput struct {
	ctx           context.Context
	target        WindowIdentity
	guard         func(context.Context) error
	keyboardReady func() error
	receipt       InputReceipt
	down          []uint16
}

// withNativeInput reuses production identity checks and its cross-process mutex.
// guard is the caller's disk ownership check, also called before every message.
func withNativeInput(ctx context.Context, target WindowIdentity, guard func(context.Context) error, keyboardReady func() error, fn func(*nativeInput) error) (InputReceipt, error) {
	if guard == nil || keyboardReady == nil || fn == nil {
		return InputReceipt{}, errors.New("desktop.invalid_options")
	}
	ctx, cancel := context.WithTimeout(ctx, 60*time.Second)
	defer cancel()
	return withInputLock(ctx, target, func() (receipt InputReceipt, err error) {
		in := &nativeInput{ctx: ctx, target: target, guard: guard, keyboardReady: keyboardReady}
		defer func() { err = errors.Join(err, in.release()); receipt = in.receipt }()
		if err = in.check(); err != nil {
			return receipt, err
		}
		if err = in.keyboardReady(); err != nil {
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
		return fmt.Errorf("desktop.post: %w", err)
	}
	in.receipt.MessagesQueued++
	return nil
}

func (in *nativeInput) key(vk uint16, up, alt bool) error {
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
			err = errors.Join(err, fmt.Errorf("desktop.release: %w", sendErr))
		} else {
			in.receipt.MessagesQueued++
		}
		if vk == 0x12 {
			alt = false
		}
	}
	in.down = nil
	return err
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
	// Recheck after the caller's fresh INPUT read, immediately before the
	// first key-down. Once started, finish the chord and release every key;
	// keyboard activity must never insert a pause with our modifiers held.
	if err := in.keyboardReady(); err != nil {
		return err
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
