//go:build windows && amd64

package channel

import (
	"errors"
	"sync"
)

// Fixed invocation aggregates only: no payloads, addresses, absolute clocks or
// raw error text. Accepted means a healthy pre-gate or an agreeing post-gate.
type InputOpticalGateDiagnostics struct {
	Checks      uint64 `json:"checks"`
	Accepted    uint64 `json:"accepted"`
	Unavailable uint64 `json:"unavailable"`
	Waiting     uint64 `json:"waiting"`
	Blocked     uint64 `json:"blocked"`
	Unknown     uint64 `json:"unknown"`
	Changed     uint64 `json:"changed"`
	Other       uint64 `json:"other"`
}

type InputOpticalDiagnostics struct {
	Pre  InputOpticalGateDiagnostics `json:"pre"`
	Post InputOpticalGateDiagnostics `json:"post"`
}

type nativeInputOpticalCollector struct {
	mu          sync.Mutex
	diagnostics InputOpticalDiagnostics
}

func (c *nativeInputOpticalCollector) Observe(post bool, err error) {
	c.mu.Lock()
	defer c.mu.Unlock()
	d := &c.diagnostics.Pre
	if post {
		d = &c.diagnostics.Post
	}
	d.Checks++
	if err == nil {
		d.Accepted++
		return
	}
	var pending *inputSignalPending
	if errors.As(err, &pending) {
		switch pending.reason {
		case "input_signal_unavailable":
			d.Unavailable++
		case "input_signal_waiting":
			d.Waiting++
		case "input_combat_lockdown":
			d.Blocked++
		case "input_signal_unknown":
			d.Unknown++
		case "input_signal_changed":
			d.Changed++
		default:
			d.Other++
		}
	} else {
		d.Other++
	}
}

func (c *nativeInputOpticalCollector) Snapshot() InputOpticalDiagnostics {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.diagnostics
}
