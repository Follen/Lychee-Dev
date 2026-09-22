// Package desktop provides native desktop primitives without game semantics.
package desktop

import "errors"

var ErrUnsupported = errors.New("desktop.unsupported_platform")
var ErrIdentityChanged = errors.New("desktop.identity_changed")

// WindowIdentity includes the process creation counter, not just a reusable PID.
// Numeric OS handles are encoded as strings to preserve precision in JSON tools.
type WindowIdentity struct {
	Handle           uint64 `json:"handle,string"`
	ProcessID        uint32 `json:"processId"`
	ProcessStartedAt uint64 `json:"processStartedAt,string"`
	Executable       string `json:"executable"`
	Class            string `json:"class"`
	Title            string `json:"title"`
}
