//go:build !windows || !amd64

package desktop

import (
	"context"
	"image"
)

func CaptureFrames(context.Context, WindowIdentity, image.Rectangle) (*FrameStream, error) {
	return nil, ErrUnsupported
}
