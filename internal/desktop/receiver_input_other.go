//go:build !windows || !amd64

package desktop

import "context"

type ReceiverInput struct{}

func WithReceiverInput(context.Context, WindowIdentity, func(context.Context) error, func(*ReceiverInput) error) (InputReceipt, error) {
	return InputReceipt{}, ErrUnsupported
}
func WithReceiverInputProfile(context.Context, WindowIdentity, ReceiverBindings, func(context.Context) error, func(*ReceiverInput) error) (InputReceipt, error) {
	return InputReceipt{}, ErrUnsupported
}

func (*ReceiverInput) SetBindings(ReceiverBindings) error { return ErrUnsupported }
func (*ReceiverInput) Wake() error                        { return ErrUnsupported }
func (*ReceiverInput) Escape() error                      { return ErrUnsupported }
func (*ReceiverInput) Submit() error                      { return ErrUnsupported }
func (*ReceiverInput) Dismiss() error                     { return ErrUnsupported }
func (*ReceiverInput) Stage(string) error                 { return ErrUnsupported }
func (*ReceiverInput) Commit(string) error                { return ErrUnsupported }
func (*ReceiverInput) FixedReload(func(int) error) error  { return ErrUnsupported }
func (*ReceiverInput) ReadyReload() error                 { return ErrUnsupported }
