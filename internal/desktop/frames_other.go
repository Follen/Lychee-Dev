//go:build !windows || !amd64

package desktop

import (
	"context"
	"image"
	"time"
)

func CaptureFrames(context.Context, WindowIdentity, image.Rectangle) (*FrameStream, error) {
	return nil, ErrUnsupported
}

func CaptureFramesWithStartupTimeout(context.Context, WindowIdentity, image.Rectangle, time.Duration) (*FrameStream, error) {
	return nil, ErrUnsupported
}
