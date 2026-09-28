//go:build windows && amd64

package channel

import (
	"context"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"path/filepath"
	"sort"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/delivery"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/live/memory"
	"github.com/follenfang/lycheedev/internal/vault"
)

type Native struct {
	Target                               desktop.WindowIdentity
	Parent, Version, Consumer, CacheFile string
	Guard                                func(context.Context) error
	Process                              *memory.Process
	Hints                                *memory.Hints
	Publication                          *vault.Lease
	Lookups                              []memory.LookupResult
	TraceDir                             string
	traceSequence                        int
}

func (n *Native) Observe(ctx context.Context, q ObservationQuery) (Observation, error) {
	return ObserveRecords(ctx, n, q)
}

func OpenNative(target desktop.WindowIdentity, parent, version, cacheFile string, cache bool) (*Native, error) {
	process, err := memory.Open(target.ProcessID, target.ProcessStartedAt, target.Executable)
	if err != nil {
		return nil, err
	}
	n := &Native{Target: target, Parent: parent, Version: version, Process: process, CacheFile: cacheFile, Consumer: fmt.Sprintf("%d/%d", target.ProcessID, target.ProcessStartedAt)}
	if cache {
		n.Hints = memory.LoadHints(cacheFile, fmt.Sprintf("%s/%s/%s/memory5", n.Consumer, target.Executable, version))
	}
	return n, nil
}
func (n *Native) Close() error {
	var err error
	if n.Publication != nil {
		err = n.Publication.Close()
		n.Publication = nil
	}
	return errors.Join(err, n.Process.Close())
}
func (n *Native) Find(ctx context.Context, s memory.Selector, first bool) (memory.LookupResult, error) {
	if n.TraceDir != "" && n.traceSequence >= 256 {
		return memory.LookupResult{}, errors.New("live.channel_observation_budget")
	}
	found, err := memory.Find(ctx, n.Process, s, n.Hints, first)
	// Coverage/latency persist in host evidence without duplicating large bodies.
	evidence := found
	evidence.Records = nil
	if len(n.Lookups) < 256 {
		n.Lookups = append(n.Lookups, evidence)
	}
	if n.TraceDir != "" {
		n.traceSequence++
		trace := struct {
			Nonce, Runtime, Ticket string
			Kind                   bridge.MemoryKind
			Matches                int
			Lookup                 memory.LookupResult
		}{hex.EncodeToString(s.Nonce[:]), hex.EncodeToString(s.Runtime[:]), hex.EncodeToString(s.Ticket[:]), s.Kind, len(found.Records), evidence}
		// Preserve coverage even when the caller's scan deadline just expired.
		traceCtx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
		traceErr := writeProjectJSON(traceCtx, filepath.Join(n.TraceDir, fmt.Sprintf("%03d.json", n.traceSequence)), trace)
		cancel()
		err = errors.Join(err, traceErr)
	}
	if n.Hints != nil {
		_ = n.Hints.Save(ctx, n.CacheFile)
	}
	return found, err
}
func (n *Native) lockPublication(ctx context.Context) error {
	if n.Publication != nil {
		return nil
	}
	if _, bounded := ctx.Deadline(); !bounded {
		return errors.New("live.channel_deadline_required")
	}
	lease, err := vault.AcquireLease(ctx, filepath.Join(n.Parent, ".lycheedev-slot-locks"), "publication")
	if err != nil {
		if ctx.Err() != nil {
			return errors.Join(ErrPublicationPending, err)
		}
		return err
	}
	n.Publication = lease
	return nil
}
func (n *Native) unlockPublication() error {
	if n.Publication == nil {
		return nil
	}
	err := n.Publication.Close()
	n.Publication = nil
	return err
}
func (n *Native) Publish(ctx context.Context, e bridge.SlotEnvelope) (err error) {
	if n.Guard == nil {
		return errors.New("live.channel_guard_required")
	}
	if err := n.Guard(ctx); err != nil {
		return err
	}
	if err := n.lockPublication(ctx); err != nil {
		return err
	}
	defer func() { err = errors.Join(err, n.unlockPublication()) }()
	// Another instance may have progressed while this driver waited. Revalidate
	// after admission, before changing the shared file.
	if err := n.Guard(ctx); err != nil {
		return err
	}
	err = delivery.PublishSlot(ctx, n.Parent, n.Version, n.Consumer, e)
	if errors.Is(err, delivery.ErrSlotReserved) {
		// The other CLI may have exited. Its durable reservation, not this OS
		// lease, prevents overwrite. Release the lease so its owner can resume.
		return ErrPublicationPending
	}
	return err
}

// RetryFirstBinding is deliberately limited to a pristine pool containing only
// this non-business bind. Even an unexpected extra wake cannot load business
// code. General input replay needs current-slot evidence and is not allowed.
func (n *Native) RetryFirstBinding(ctx context.Context, e bridge.SlotEnvelope) (err error) {
	if e.Action != "bind" || e.Index != 1 || e.Code != "" {
		return errors.New("live.channel_bootstrap_retry_unsafe")
	}
	if err := n.Publish(ctx, e); err != nil {
		return err
	}
	if err := n.lockPublication(ctx); err != nil {
		return err
	}
	defer func() { err = errors.Join(err, n.unlockPublication()) }()
	pool, err := delivery.InspectSlots(ctx, n.Parent, n.Version)
	if err != nil {
		return err
	}
	for i, slot := range pool.Files {
		if i == 0 {
			if slot.Nonce != e.Nonce || slot.Consumer != n.Consumer || slot.Consumed {
				return errors.New("live.channel_bootstrap_retry_unsafe")
			}
		} else if slot.Nonce != "" || slot.PendingHash != "" {
			return errors.New("live.channel_bootstrap_retry_unsafe")
		}
	}
	s, err := n.ObserveInput(ctx, e, 0)
	if err != nil {
		return err
	}
	_, err = n.Input(ctx, InputAction{Kind: "invoke", Envelope: e, Observation: &s})
	return err
}
func (n *Native) Consumed(ctx context.Context, e bridge.SlotEnvelope) (err error) {
	if err := n.lockPublication(ctx); err != nil {
		return err
	}
	defer func() { err = errors.Join(err, n.unlockPublication()) }()
	if err := delivery.ConfirmSlotConsumed(ctx, n.Parent, n.Version, n.Consumer, e.Index, e.Nonce); err != nil {
		return err
	}
	return nil
}

func (n *Native) Supersede(ctx context.Context, e bridge.SlotEnvelope, current Identity) error {
	if current.Owner != e.Owner || current.GUID != e.GUID || current.Build != e.Build || current.Runtime == e.Runtime {
		return errors.New("live.channel_runtime_retirement_invalid")
	}
	if err := n.Guard(ctx); err != nil {
		return err
	}
	if err := n.lockPublication(ctx); err != nil {
		return err
	}
	err := delivery.RetireSlotRuntime(ctx, n.Parent, n.Version, n.Consumer, e, current.Runtime)
	closeErr := n.Publication.Close()
	n.Publication = nil
	return errors.Join(err, closeErr)
}

// Discover returns untrusted candidates only. Bind's fresh nonce proves the
// selected runtime. A scan is never authority to execute a business script.
func (n *Native) Discover(ctx context.Context, character, realm string) ([]Identity, memory.Coverage, error) {
	found, err := n.Find(ctx, memory.Selector{Kind: bridge.MemoryIdentity}, false)
	if err != nil {
		return nil, found.Coverage, err
	}
	identities := []Identity{}
	seen := map[string]bool{}
	for _, record := range found.Records {
		var i Identity
		if json.Unmarshal(record.Payload, &i) != nil || i.Schema != "lycheedev.slot.identity.v1" || i.Validate() != nil || i.Release != n.Version {
			continue
		}
		runtime, _ := tokenBytes(i.Runtime)
		if runtime != record.Header.Runtime || (character != "" && i.Character != character) || (realm != "" && i.Realm != realm) {
			continue
		}
		key := fmt.Sprintf("%s/%d", i.Runtime, i.NextSlot)
		if !seen[key] {
			identities = append(identities, i)
			seen[key] = true
		}
	}
	sort.Slice(identities, func(a, b int) bool {
		if identities[a].Runtime == identities[b].Runtime {
			return identities[a].NextSlot > identities[b].NextSlot
		}
		return identities[a].Runtime > identities[b].Runtime
	})
	return identities, found.Coverage, nil
}
