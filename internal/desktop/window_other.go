//go:build !windows

package desktop

import "context"

func ListWindows(context.Context) ([]WindowIdentity, error) { return nil, ErrUnsupported }
func ConfirmWindow(context.Context, WindowIdentity) error   { return ErrUnsupported }
