//go:build windows && amd64

package desktop

import (
	"context"
	"errors"
	"strings"
	"time"
)

// ReceiverInput holds the window input mutex throughout a receiver transaction.
// Queued messages are attempts only; the caller must verify optical receipts.
type ReceiverInput struct {
	native   *nativeInput
	bindings ReceiverBindings
}

// WithReceiverInput rechecks the selected window and caller's durable ownership
// before every message. It never changes foreground focus or falls back to chat.
func WithReceiverInput(ctx context.Context, target WindowIdentity, guard func(context.Context) error, fn func(*ReceiverInput) error) (InputReceipt, error) {
	return WithReceiverInputProfile(ctx, target, DefaultReceiverBindings(), guard, fn)
}

func WithReceiverInputProfile(ctx context.Context, target WindowIdentity, bindings ReceiverBindings, guard func(context.Context) error, fn func(*ReceiverInput) error) (InputReceipt, error) {
	if guard == nil || fn == nil {
		return InputReceipt{}, errors.New("desktop.receiver_callbacks_required")
	}
	if err := ValidateReceiverBindings(bindings); err != nil {
		return InputReceipt{}, err
	}
	return withNativeInput(ctx, target, "postmessage", guard, func(native *nativeInput) error {
		return fn(&ReceiverInput{native: native, bindings: bindings})
	})
}

func (in *ReceiverInput) SetBindings(bindings ReceiverBindings) error {
	if err := ValidateReceiverBindings(bindings); err != nil {
		return err
	}
	in.bindings = bindings
	return nil
}

func (in *ReceiverInput) Wake() error    { return in.native.ReceiverChord(in.bindings.WakeBinding) }
func (in *ReceiverInput) Submit() error  { return in.native.ReceiverChord(in.bindings.SubmitBinding) }
func (in *ReceiverInput) Dismiss() error { return in.native.ReceiverChord(in.bindings.CloseBinding) }

func receiverWire(text, prefix string, limit int) error {
	if len(text) == 0 || len(text) > limit || !strings.HasPrefix(text, prefix) {
		return errors.New("desktop.receiver_wire_invalid")
	}
	for i := 0; i < len(text); i++ {
		if text[i] < 33 || text[i] > 126 {
			return errors.New("desktop.receiver_wire_invalid")
		}
	}
	return nil
}

// Stage is deliberately capped below the addon's 256-byte parser maximum:
// 50 ms per character plus the commit frame and optical readback must fit the
// receiver's fixed 20-second deadline. A longer legal wire needs separately
// demonstrated faster input cadence before it can be sent.
func (in *ReceiverInput) Stage(frame string) error {
	if err := receiverWire(frame, "LDB1:", 180); err != nil {
		return err
	}
	return in.native.Text(frame)
}

func (in *ReceiverInput) Commit(frame string) error {
	if err := receiverWire(frame, "LDC1:", 128); err != nil {
		return err
	}
	if err := in.native.Text(frame); err != nil {
		return err
	}
	// intent-v2 dispatches only on a key event, after the complete commit frame
	// has been checked. The caller durably records commit intent before both.
	return in.native.Chord("enter")
}

// FixedReload is the sole sessionless chat primitive. before is called before
// each step so durable evidence can record even an unknown partial send. The
// text step may partially queue /reload; retrying it is never implied.
func (in *ReceiverInput) FixedReload(before func(step int) error) error {
	if before == nil {
		return errors.New("desktop.reload_progress_required")
	}
	step := func(n int, send func() error) error {
		if err := before(n); err != nil {
			return err
		}
		return send()
	}
	for n := 1; n <= 3; n++ {
		if err := step(n, func() error { return in.native.Chord("escape") }); err != nil {
			return err
		}
		if err := in.native.Wait(150 * time.Millisecond); err != nil {
			return err
		}
	}
	if err := step(4, func() error { return in.native.Chord("enter") }); err != nil {
		return err
	}
	if err := in.native.Wait(150 * time.Millisecond); err != nil {
		return err
	}
	if err := step(5, func() error { return in.native.Text("/reload") }); err != nil {
		return err
	}
	return step(6, func() error { return in.native.Chord("enter") })
}
