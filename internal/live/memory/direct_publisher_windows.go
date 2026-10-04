//go:build windows && amd64

package memory

import (
	"context"
	"errors"

	"github.com/follenfang/lycheedev/internal/live/duplex"
)

// PublishDirectDuplexRow never attaches a debugger or suspends a target thread.
// It preserves the exact one-row publication and its readback, but a game
// reload between the final gate and WPM can invalidate the Lua allocation.
// This route is only enabled for the owner's exact-image trial profile.
func PublishDirectDuplexRow(ctx context.Context, request StoppedPublicationRequest) (duplex.WriteOutcome, []DuplexWriteRange, StoppedObservation, error) {
	noWrite := duplex.WriteOutcome{State: duplex.NoWrite}
	noStop := StoppedObservation{}
	if _, ok := ctx.Deadline(); !ok {
		return noWrite, nil, noStop, errors.New("memory.write_deadline_required")
	}
	if err := ctx.Err(); err != nil {
		return noWrite, nil, noStop, err
	}
	profile := DuplexWriteCapability(request.ExecutableSHA256, request.Build, request.Product)
	if !profile.Eligible || profile.Mode != "direct" {
		return noWrite, nil, noStop, errors.New("live.duplex_writer_profile_unverified")
	}
	if request.Release == "" || request.ActorGUID == "" {
		return noWrite, nil, noStop, duplex.ErrIdentity
	}
	if _, err := duplex.EncodeMessage(request.Message); err != nil {
		return noWrite, nil, noStop, err
	}
	p, err := OpenDuplexWriter(request.Target.PID, request.Target.Created, request.Target.Image)
	if err != nil {
		return noWrite, nil, noStop, err
	}
	defer p.Close()
	module, err := p.MainModule(ctx)
	if err != nil {
		return noWrite, nil, noStop, err
	}
	if module.ExecutableSHA256 != request.ExecutableSHA256 {
		return noWrite, nil, noStop, errors.New("memory.image_changed")
	}
	readModule := func(c context.Context, at uint64, b []byte) (int, error) { return p.ReadModule(c, module, at, b) }
	reload, err := ResolveReloadState(ctx, module.Base, module.Size, module.ExecutableSHA256, request.Build, request.Product, module.LuaImageLayout(), readModule)
	if err != nil {
		return noWrite, nil, noStop, err
	}
	reader, err := OpenLuaMailbox(ctx, p, module.Base, module.Size, module.ExecutableSHA256, request.Release, module.LuaImageLayout(), readModule)
	if err != nil {
		return noWrite, nil, noStop, err
	}
	guard := func(c context.Context) error {
		if err := p.Verify(c); err != nil {
			return err
		}
		observation, err := reload.Observe(c)
		if err != nil {
			return err
		}
		if request.Message.Header.Kind == duplex.Frame {
			err = observation.CheckBusinessWriteGate()
		} else {
			err = observation.CheckWriteGate()
		}
		if err != nil {
			return err
		}
		wire, err := reader.ReadDuplexString(c, []DuplexPath{{Name: "sendbox"}, {Name: "status"}}, duplex.MaxSendboxBytes+44)
		if err != nil {
			return err
		}
		s, err := duplex.DecodeSendbox(wire)
		if err != nil {
			return err
		}
		if s.Build != request.Build || s.Product != request.Product || s.Release != request.Release {
			return duplex.ErrIdentity
		}
		h := request.Message.Header
		if s.Runtime != h.Runtime || s.Arena != h.Arena || s.ActorBinding != h.ActorBinding || s.ActorGUID != request.ActorGUID {
			return duplex.ErrIdentity
		}
		if h.Kind == duplex.Frame {
			return duplex.ValidateFrameTransition(s, request.Message)
		}
		matched := s.Session == h.Session && s.Owner == h.Owner && s.Fence == h.Fence
		if !matched {
			candidate, err := duplex.NextIdentity(s, h.Owner, h.Session)
			matched = err == nil && candidate == (duplex.Identity{Runtime: h.Runtime, Arena: h.Arena, Session: h.Session, Owner: h.Owner, ActorBinding: h.ActorBinding, Fence: h.Fence}) && (s.ReadyChallenge == h.Challenge || h.Kind == duplex.Repair && s.Repair != nil && s.Repair.Challenge == h.Challenge)
		}
		if !matched {
			return duplex.ErrIdentity
		}
		if !s.ControlReady {
			return duplex.ErrPending
		}
		return nil
	}
	out, facts, err := p.PublishDuplexRow(ctx, reader, request.Message, guard)
	return out, facts, noStop, err
}
