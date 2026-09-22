//go:build !windows

package desktop

import "context"

func QueueCommand(context.Context, WindowIdentity, string) (InputReceipt, error) {
	return InputReceipt{}, ErrUnsupported
}

func queueBootstrapCommand(context.Context, WindowIdentity, string) (InputReceipt, error) {
	return InputReceipt{}, ErrUnsupported
}

func QueuePreparedCommand(context.Context, WindowIdentity, func(context.Context) (string, error), func(context.Context) error) (InputReceipt, error) {
	return InputReceipt{}, ErrUnsupported
}
