//go:build windows && amd64

package desktop

import (
	"context"
	"errors"
)

// WithMailboxInput targets one window without changing foreground focus.
// Each callback sends a chord; callers still verify the resulting Mailbox.
func WithMailboxInput(ctx context.Context, target WindowIdentity, b MailboxBindings, guard func(context.Context) error, fn func(wake, reload func() error) error) (InputReceipt, error) {
	if guard == nil || fn == nil {
		return InputReceipt{}, errors.New("desktop.mailbox_callbacks_required")
	}
	if err := ValidateMailboxBindings(b); err != nil {
		return InputReceipt{}, err
	}
	return withNativeInput(ctx, target, "postmessage", guard, func(in *nativeInput) error {
		return fn(func() error { return in.ReceiverChord(b.Wake) },
			func() error { return in.ReceiverChord(b.Reload) })
	})
}
