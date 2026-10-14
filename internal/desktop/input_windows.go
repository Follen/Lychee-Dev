//go:build windows

package desktop

var (
	postWindowMessage = userLibrary.NewProc("PostMessageW")
	mapVirtualKey     = userLibrary.NewProc("MapVirtualKeyW")
	windowMinimized   = userLibrary.NewProc("IsIconic")
)
