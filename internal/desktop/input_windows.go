//go:build windows

package desktop

import (
	"context"
	"errors"
	"fmt"
	"time"
)

var (
	postWindowMessage = userLibrary.NewProc("PostMessageW")
	mapVirtualKey     = userLibrary.NewProc("MapVirtualKeyW")
	windowMinimized   = userLibrary.NewProc("IsIconic")
)

// QueueCommand requires the bridge to have already verified fresh input-ready
// evidence. It acquires a non-waiting cross-process window input mutex and never activates a
// window, resets arbitrary edit controls, reads the clipboard, or retries text.
func QueueCommand(parent context.Context, target WindowIdentity, command string) (InputReceipt, error) {
	if _, err := commandUnits(command); err != nil {
		return InputReceipt{}, err
	}
	return withInputLock(parent, target, func() (InputReceipt, error) { return queueCommand(parent, target, command, nil) })
}

// queueBootstrapCommand sends one already allowlisted bootstrap string under
// the same cross-process input mutex, per-message identity recheck, minimized
// check and bounded context as QueueCommand.
func queueBootstrapCommand(parent context.Context, target WindowIdentity, command string) (InputReceipt, error) {
	return withInputLock(parent, target, func() (InputReceipt, error) { return queueCommand(parent, target, command, nil) })
}

// QueuePreparedCommand holds the input mutex while the caller obtains fresh
// eligibility and records dispatch intent. The guard runs before every queued
// message; it must check ownership, not require the pre-input QR to stay visible.
func QueuePreparedCommand(parent context.Context, target WindowIdentity, prepare func(context.Context) (string, error), guard func(context.Context) error) (InputReceipt, error) {
	if prepare == nil || guard == nil {
		return InputReceipt{}, errors.New("desktop.input_callbacks_required")
	}
	ctx, cancel := context.WithTimeout(parent, 40*time.Second)
	defer cancel()
	return withInputLock(ctx, target, func() (InputReceipt, error) {
		if err := guard(ctx); err != nil {
			return InputReceipt{}, err
		}
		command, err := prepare(ctx)
		if err != nil {
			return InputReceipt{}, err
		}
		return queueCommand(ctx, target, command, guard)
	})
}

func queueCommand(parent context.Context, target WindowIdentity, command string, guard func(context.Context) error) (InputReceipt, error) {
	var receipt InputReceipt
	units, err := commandUnits(command)
	if err != nil {
		return receipt, err
	}
	ctx, cancel := context.WithTimeout(parent, 20*time.Second)
	defer cancel()
	scan, _, _ := mapVirtualKey.Call(13, 0)
	keyBits := uintptr(1) | (scan << 16)
	post := func(message uint32, value, bits uintptr) error {
		if guard != nil {
			if err := guard(ctx); err != nil {
				return err
			}
		}
		if err := ConfirmWindow(ctx, target); err != nil {
			return err
		}
		minimized, _, _ := windowMinimized.Call(uintptr(target.Handle))
		if minimized != 0 {
			return fmt.Errorf("desktop.window_minimized")
		}
		ok, _, err := postWindowMessage.Call(uintptr(target.Handle), uintptr(message), value, bits)
		if ok == 0 {
			return fmt.Errorf("desktop.post_message: %w", err)
		}
		receipt.MessagesQueued++
		return nil
	}
	key := func() error {
		if err := post(0x100, 13, keyBits); err != nil {
			return err
		}
		if err := post(0x101, 13, keyBits|1<<30|1<<31); err != nil {
			// Release only our Return key, only if the same identity still exists.
			// An Escape reset could erase someone else's draft and is forbidden.
			if ConfirmWindow(context.WithoutCancel(ctx), target) == nil {
				postWindowMessage.Call(uintptr(target.Handle), 0x101, 13, keyBits|1<<30|1<<31)
			}
			return err
		}
		return nil
	}
	wait := func(duration time.Duration) error {
		timer := time.NewTimer(duration)
		defer timer.Stop()
		select {
		case <-timer.C:
			return nil
		case <-ctx.Done():
			return ctx.Err()
		}
	}
	if err := key(); err != nil {
		return receipt, err
	}
	if err := wait(150 * time.Millisecond); err != nil {
		return receipt, err
	}
	for _, unit := range units {
		if err := post(0x102, uintptr(unit), 1); err != nil {
			return receipt, err
		}
		if err := wait(10 * time.Millisecond); err != nil {
			return receipt, err
		}
	}
	if err := key(); err != nil {
		return receipt, err
	}
	receipt.SubmissionComplete = true
	return receipt, nil
}
