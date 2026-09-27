//go:build windows && amd64 && lycheedev_input_lab

package desktop

import (
	"context"
	"fmt"
	"unsafe"
)

// LabInput exposes the production PostMessage sender to the isolated lab. Its
// explicit foreground SendInput mode is compiled only with the lab tag.
type LabInput struct{ *nativeInput }

func WithLabInput(ctx context.Context, target WindowIdentity, mode string, guard func(context.Context) error, fn func(*LabInput) error) (InputReceipt, error) {
	return withNativeInput(ctx, target, mode, guard, func(in *nativeInput) error { return fn(&LabInput{in}) })
}

// HideLabViaChat is fixture cleanup only; the production receiver has no
// caller-supplied chat command or arbitrary-text entry point.
func (in *LabInput) HideLabViaChat() error { return in.resetAndEnter("/lycheeinputlab hide") }

// Win64 INPUT is 40 bytes; the keyboard union starts at byte 8.
type labNativeInput struct {
	Kind        uint32
	Pad         uint32
	Key, Scan   uint16
	Flags, Time uint32
	Pad2        uint32
	Extra       uint64
	Tail        uint64
}

func init() {
	labSendPacket = func(in *nativeInput, vk, scan uint16, flags uint32) error {
		packet := labNativeInput{Kind: 1, Key: vk, Scan: scan, Flags: flags}
		n, _, err := userLibrary.NewProc("SendInput").Call(1, uintptr(unsafe.Pointer(&packet)), unsafe.Sizeof(packet))
		if n != 1 {
			return fmt.Errorf("lab.sendinput: %w", err)
		}
		in.receipt.MessagesQueued++
		return nil
	}
}

func LabKeyboardLayout(target WindowIdentity) string {
	tid, _, _ := windowProcess.Call(uintptr(target.Handle), 0)
	hkl, _, _ := userLibrary.NewProc("GetKeyboardLayout").Call(tid)
	return fmt.Sprintf("0x%x", hkl)
}

func LabForeground(target WindowIdentity) bool {
	hwnd, _, _ := userLibrary.NewProc("GetForegroundWindow").Call()
	return hwnd == uintptr(target.Handle)
}
