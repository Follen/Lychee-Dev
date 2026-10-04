package memory

import (
	"bytes"
	"context"
	"encoding/binary"
	"errors"
)

var ErrReloadGuardRequired = errors.New("memory.write_lifecycle_guard_required")
var ErrReloadActive = errors.New("live.duplex_runtime_reloading")
var ErrReloadUnknown = errors.New("live.duplex_reload_state_unavailable")
var ErrWorldUnknown = errors.New("live.duplex_world_state_unavailable")
var ErrNotInWorld = errors.New("live.duplex_character_not_in_world")

type ReloadObservation struct {
	RecipeID string `json:"recipeId,omitempty"`
	State    string `json:"state"`
	Reason   string `json:"reason,omitempty"`
	// Requested is Glue pending; GameUI requests use mode bit 7.
	Requested   *uint8            `json:"glueRequest,omitempty"`
	Mode        *uint16           `json:"modeFlags,omitempty"`
	WorldReady  *bool             `json:"worldReady,omitempty"`
	WorldState  string            `json:"worldState"`
	WorldReason string            `json:"worldReason,omitempty"`
	Resolution  *ReloadResolution `json:"resolution,omitempty"`
	proof       *reloadObservationProof
}
type reloadObservationProof struct {
	recipe, state, worldState string
	request                   uint8
	mode                      uint16
	world                     *bool
	resolution                ReloadResolution
}

// A read gate is not allocation ownership or VM liveness. Passing it neither
// grants write capability nor closes the final observation-to-write race.
// The private snapshot rejects decoded or edited diagnostic projections.
func (o ReloadObservation) CheckWriteGate() error {
	p := o.proof
	if p == nil || o.RecipeID != p.recipe || o.State != p.state || o.Requested == nil || o.Mode == nil || *o.Requested != p.request || *o.Mode != p.mode || o.Resolution == nil || *o.Resolution != p.resolution || o.WorldState != p.worldState || (o.WorldReady == nil) != (p.world == nil) || o.WorldReady != nil && *o.WorldReady != *p.world {
		return ErrReloadUnknown
	}
	switch o.State {
	case "requested", "reloading":
		return ErrReloadActive
	case "no_reload_observed":
		return nil
	default:
		return ErrReloadUnknown
	}
}

// World admission applies only to new business. Cleanup controls use the
// reload gate so leaving the world cannot strand ACK/cancel/close obligations.
func (o ReloadObservation) CheckBusinessWriteGate() error {
	if e := o.CheckWriteGate(); e != nil {
		return e
	}
	if o.proof.world == nil {
		return ErrWorldUnknown
	}
	if !*o.proof.world {
		return ErrNotInWorld
	}
	return nil
}

// Observe rechecks locating metadata and both complete reload instruction
// anchors around repeated flag samples. Bit 8 covers close/recreate after bit 7
// clears. Bit 1 and bit 9 remain mode candidates, not VM-liveness proofs.
// WorldReady requires its separately verified IsPlayerInWorld getter anchor.
func (b *ReloadBinding) Observe(ctx context.Context) (ReloadObservation, error) {
	o := ReloadObservation{State: "unknown", WorldState: "unknown"}
	fail := func(reason string, cause error) (ReloadObservation, error) {
		o.Reason = reason
		return o, errors.Join(ErrReloadUnknown, cause)
	}
	if b == nil || !b.valid() {
		return fail("reload binding proof unavailable", nil)
	}
	resolution := b.Resolution
	o.RecipeID, o.Resolution = resolution.RecipeID, &resolution
	p := b.proof
	r := &luaRootModule{ctx: ctx, base: p.base, size: p.size, read: p.read}
	if e := p.verify(r); e != nil {
		return fail("reload locator changed before sample", e)
	}
	worldValid := len(p.world) > 0
	if worldValid {
		if e := p.verifyWorld(r); e != nil {
			worldValid = false
			o.WorldReason = e.Error()
		}
	} else {
		o.WorldReason = resolution.Evidence.WorldQualification
	}
	var request, requestAfter [1]byte
	var mode, modeAfter [2]byte
	if e := r.exact(resolution.NormalRVA, request[:]); e != nil {
		return fail("reload request unreadable", e)
	}
	if e := r.exact(resolution.ModeRVA, mode[:]); e != nil {
		return fail("reload mode unreadable", e)
	}
	if e := r.exact(resolution.NormalRVA, requestAfter[:]); e != nil {
		return fail("reload request recheck failed", e)
	}
	if e := r.exact(resolution.ModeRVA, modeAfter[:]); e != nil {
		return fail("reload mode recheck failed", e)
	}
	if !bytes.Equal(request[:], requestAfter[:]) || !bytes.Equal(mode[:], modeAfter[:]) {
		return fail("reload state changed while sampled", nil)
	}
	if e := p.verify(r); e != nil {
		return fail("reload locator changed while sampled", e)
	}
	if worldValid {
		if e := p.verifyWorld(r); e != nil {
			worldValid = false
			o.WorldReason = e.Error()
		}
	}
	rawRequest, rawMode := request[0], binary.LittleEndian.Uint16(mode[:])
	o.Requested, o.Mode = &rawRequest, &rawMode
	if rawRequest > 1 {
		return fail("reload request outside observed contract", nil)
	}
	switch {
	case rawMode&(1<<8) != 0:
		o.State = "reloading"
	case rawRequest != 0 || rawMode&(1<<7) != 0:
		o.State = "requested"
	case rawMode&(1<<1) != 0:
		o.State = "teardown_candidate"
	case rawMode&(1<<9) == 0:
		o.State = "mode_inactive_candidate"
	default:
		o.State = "no_reload_observed"
	}
	var privateWorld *bool
	if worldValid {
		world := rawMode&(1<<4) != 0
		o.WorldReady = &world
		copyWorld := world
		privateWorld = &copyWorld
		if world {
			o.WorldState = "world_ready"
		} else {
			o.WorldState = "not_in_world"
		}
	}
	o.proof = &reloadObservationProof{o.RecipeID, o.State, o.WorldState, rawRequest, rawMode, privateWorld, resolution}
	return o, nil
}

// Repeated gates should retain ResolveReloadState's binding instead of rescanning.
func ReadReloadObservation(ctx context.Context, base, size uint64, hash, build, product string, layout LuaRootImageLayout, readModule func(context.Context, uint64, []byte) (int, error)) (ReloadObservation, error) {
	b, e := ResolveReloadState(ctx, base, size, hash, build, product, layout, readModule)
	if e != nil {
		return ReloadObservation{State: "unknown", WorldState: "unknown", Reason: e.Error()}, e
	}
	return b.Observe(ctx)
}
