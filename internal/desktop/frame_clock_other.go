//go:build !windows || !amd64

package desktop

func CaptureSystemTicks() (int64, error) { return 0, ErrUnsupported }
