//go:build windows && amd64

package duplexhost

import (
	"context"

	"github.com/follenfang/lycheedev/internal/live"
	"github.com/follenfang/lycheedev/internal/live/memory"
)

// ObserveTargetReload needs no loaded addon or Lua-heap traversal. This keeps
// doctor useful while reload has removed the addon runtime from the old VM.
func ObserveTargetReload(ctx context.Context, target live.ClientWindow) (memory.ReloadObservation, error) {
	unknown := memory.ReloadObservation{State: "unknown", WorldState: "unknown"}
	p, err := memory.Open(target.Window.ProcessID, target.Window.ProcessStartedAt, target.Window.Executable)
	if err != nil {
		unknown.Reason = err.Error()
		return unknown, err
	}
	defer p.Close()
	m, err := p.MainModule(ctx)
	if err != nil {
		unknown.Reason = err.Error()
		return unknown, err
	}
	b, err := memory.ResolveReloadState(ctx, m.Base, m.Size, m.ExecutableSHA256, target.Client.FullBuild, target.Client.Product, m.LuaImageLayout(), func(c context.Context, at uint64, buf []byte) (int, error) { return p.ReadModule(c, m, at, buf) })
	if err != nil {
		unknown.Reason = err.Error()
		return unknown, err
	}
	return b.Observe(ctx)
}
