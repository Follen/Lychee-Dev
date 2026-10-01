//go:build windows && amd64

package channel

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/live/memory"
	"strings"
	"time"
)

// ObserveRuntimeReplacement uses the current named publication twice. Retained
// strings and heap searches cannot establish replacement in this protocol.
func (n *Native) ObserveRuntimeReplacement(ctx context.Context, old Identity) (*RuntimeReplacementProof, error) {
	if n.Guard == nil || n.Process == nil || n.Mailbox == nil {
		return nil, errors.New("live.channel_mailbox_required")
	}
	if n.Process.PID != n.Target.ProcessID || n.Process.Created != n.Target.ProcessStartedAt || !strings.EqualFold(n.Process.Image, n.Target.Executable) {
		return nil, errors.New("live.channel_runtime_replacement_target_changed")
	}
	ctx, cancel := context.WithTimeout(ctx, 15*time.Second)
	defer cancel()
	read := func(ctx context.Context) (Identity, memory.Record, error) {
		if err := n.Guard(ctx); err != nil {
			return Identity{}, memory.Record{}, err
		}
		candidates, _, err := n.Discover(ctx, "", "")
		if err != nil {
			return Identity{}, memory.Record{}, err
		}
		if len(candidates) != 1 {
			return Identity{}, memory.Record{}, ErrPending
		}
		current := candidates[0]
		runtime, _ := tokenBytes(current.Runtime)
		found, err := n.Find(ctx, memory.Selector{Kind: bridge.MemoryInputState, Runtime: runtime, Nonce: runtime}, true)
		if err != nil {
			return Identity{}, memory.Record{}, err
		}
		if len(found.Records) != 1 {
			return Identity{}, memory.Record{}, ErrPending
		}
		if err := n.Guard(ctx); err != nil {
			return Identity{}, memory.Record{}, err
		}
		return current, found.Records[0], nil
	}
	return observeMailboxReplacement(ctx, old, n.Target.ProcessID, n.Target.ProcessStartedAt, read, func(ctx context.Context) error {
		timer := time.NewTimer(1100 * time.Millisecond)
		defer timer.Stop()
		select {
		case <-ctx.Done():
			return ctx.Err()
		case <-timer.C:
			return nil
		}
	}, uptimeMillis)
}

func observeMailboxReplacement(ctx context.Context, old Identity, pid uint32, creation uint64, read func(context.Context) (Identity, memory.Record, error), pause func(context.Context) error, now func() int64) (*RuntimeReplacementProof, error) {
	if old.Validate() != nil || pid == 0 || creation == 0 {
		return nil, errors.New("live.channel_runtime_replacement_invalid")
	}
	eligible := func(current Identity, record memory.Record) (InputObservation, bool) {
		o, ok := replacementRecord(record)
		at := now()
		return o, ok && current.Validate() == nil && observedInputCapability(current.InputState) && current.Runtime != old.Runtime && current.Build == old.Build && current.Product == old.Product && current.Release == old.Release && o.Runtime == current.Runtime && o.GUID == current.GUID && o.Build == current.Build && o.Owner == current.Owner && o.Fence == current.Fence && o.NextSlot == current.NextSlot && o.SampleMillis >= at-500 && o.SampleMillis <= at+100
	}
	current, before, err := read(ctx)
	if err != nil {
		return nil, err
	}
	initial, ok := eligible(current, before)
	if !ok {
		return nil, nil
	}
	if err = pause(ctx); err != nil {
		return nil, errors.Join(ErrPending, err)
	}
	next, after, err := read(ctx)
	if err != nil {
		return nil, err
	}
	latest, ok := eligible(next, after)
	if !ok || next.Runtime != current.Runtime || next.GUID != current.GUID || next.Owner != current.Owner || next.Fence != current.Fence || next.NextSlot != current.NextSlot || latest.SampleMillis <= initial.SampleMillis || after.Header.Sequence <= before.Header.Sequence {
		return nil, nil
	}
	// The public root proves current reachability. A newer sample proves active
	// production; a retained old immutable record alone proves neither.
	oldWire, err := bridge.EncodeMemoryRecord(before.Header, before.Payload)
	if err != nil {
		return nil, err
	}
	newWire, err := bridge.EncodeMemoryRecord(after.Header, after.Payload)
	if err != nil {
		return nil, err
	}
	a, z := sha256.Sum256(oldWire), sha256.Sum256(newWire)
	proof := &RuntimeReplacementProof{Schema: RuntimeReplacementProofSchema, ProcessID: pid, ProcessStartedAt: creation, Current: next, Witness: RuntimeReplacementWitness{Address: after.Address, Length: uint32(len(newWire)), BeforeSHA256: hex.EncodeToString(a[:]), AfterSHA256: hex.EncodeToString(z[:]), First: initial, FirstSequence: before.Header.Sequence, Sequence: after.Header.Sequence, Observation: latest}}
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if err := proof.Validate(old); err != nil {
		return nil, err
	}
	return proof, nil
}
func replacementRecord(r memory.Record) (InputObservation, bool) {
	var o InputObservation
	h := r.Header
	if len(r.Payload) > 2048 || h.Kind != bridge.MemoryInputState || h.State != 1 || h.Sequence == 0 || h.Sequence == ^uint32(0) || h.Ticket != [16]byte{} || json.Unmarshal(r.Payload, &o) != nil {
		return o, false
	}
	runtime, err := tokenBytes(o.Runtime)
	return o, err == nil && runtime != [16]byte{} && h.Runtime == runtime && h.Nonce == runtime && o.Schema == InputSchema && validReplacementObservation(o)
}
