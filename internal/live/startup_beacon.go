package live

import (
	"errors"
	"github.com/follenfang/lycheedev/internal/desktop"
	"time"
)

var errReloadBeaconReady = errors.New("live.reload_beacon_ready")

type startupBeaconObserver struct {
	prior   desktop.StartupBeacon
	seen    bool
	changes int
	ticks   int64
	at      time.Time
}

func (o *startupBeaconObserver) Observe(frame *desktop.CapturedFrame) bool {
	if frame == nil || frame.NRGBA == nil || frame.SystemTicks <= o.ticks || time.Since(frame.ObservedAt) < 0 || time.Since(frame.ObservedAt) > time.Second {
		return false
	}
	o.ticks = frame.SystemTicks
	next, ok := desktop.ReadStartupBeacon(frame.NRGBA)
	if !ok {
		o.seen = false
		o.changes = 0
		return false
	}
	if !o.seen || next.X != o.prior.X || next.Y != o.prior.Y || next.Pitch != o.prior.Pitch || frame.ObservedAt.Sub(o.at) > 2*time.Second {
		o.prior = next
		o.seen = true
		o.changes = 0
		o.at = frame.ObservedAt
		return false
	}
	if next.Phase == o.prior.Phase {
		return false
	}
	if next.Phase != (o.prior.Phase+1)%3 {
		o.changes = 0
	} else {
		o.changes++
	}
	o.prior = next
	o.at = frame.ObservedAt
	return o.changes >= 2
}
