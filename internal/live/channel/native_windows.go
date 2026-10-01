//go:build windows && amd64

package channel

import (
	"context"
	"encoding/hex"
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
	Mailbox                              *memory.MailboxReader
	Hints                                *memory.Hints
	Publication                          *vault.Lease
	Lookups                              []memory.LookupResult
	TraceDir                             string
	traceSequence                        int
	traces                               []lookupTrace
	hintsDirty                           bool
	inputWait                            inputSampleWait
	inputHintRuntime                     string
	inputHintMillis                      int64
	inputSignal                          *nativeInputSignal
	inputAttemptProbe                    receiptAttemptProbe
}

const observationLimit = 256
const diagnosticFlushBudget = 10 * time.Second

type lookupTrace struct {
	Nonce, Runtime, Ticket string
	Kind                   bridge.MemoryKind
	Matches                int
	Lookup                 memory.LookupResult
	RootBinding            *memory.LuaRootBinding `json:"rootBinding,omitempty"`
	sequence               int
}

func (n *Native) Observe(ctx context.Context, q ObservationQuery) (Observation, error) {
	observation, err := ObserveRecords(ctx, n, q)
	if n.Mailbox == nil {
		return observation, err
	}
	return n.inputAttemptProbe.observe(ctx, n, q, observation, err, uptimeMillis())
}

func OpenNative(ctx context.Context, target desktop.WindowIdentity, parent, version, cacheFile string, cache bool) (*Native, error) {
	process, err := memory.Open(target.ProcessID, target.ProcessStartedAt, target.Executable)
	if err != nil {
		return nil, err
	}
	n := &Native{Target: target, Parent: parent, Version: version, Process: process, CacheFile: cacheFile, Consumer: fmt.Sprintf("%d/%d", target.ProcessID, target.ProcessStartedAt)}
	module, err := process.MainModule(ctx)
	if err != nil {
		_ = process.Close()
		return nil, err
	}
	n.Mailbox, err = memory.OpenLuaMailbox(ctx, process, module.Base, module.Size, module.ExecutableSHA256, version, module.LuaImageLayout(), func(ctx context.Context, address uint64, b []byte) (int, error) {
		return process.ReadModule(ctx, module, address, b)
	})
	if err != nil {
		_ = process.Close()
		if errors.Is(err, memory.ErrLuaMailboxRootUnsupported) {
			return nil, fmt.Errorf("live.channel_mailbox_build_unsupported: %w", err)
		}
		return nil, err
	}
	// Address hints do not participate in this protocol. Every read resolves
	// the current public field through the build-bound root, including cache-off.
	return n, nil
}
func (n *Native) Close() error {
	var err error
	if n.Publication != nil {
		err = n.Publication.Close()
		n.Publication = nil
	}
	if n.Process != nil {
		err = errors.Join(err, n.Process.Close())
		n.Process = nil
	}
	if n.inputSignal != nil {
		n.inputSignal.close()
		n.inputSignal = nil
	}
	// Never retain an input/publication lease while writing diagnostics. This
	// final bounded flush also runs after the command context was cancelled.
	ctx, cancel := context.WithTimeout(context.Background(), diagnosticFlushBudget)
	defer cancel()
	err = errors.Join(err, n.flushTraces(ctx, writeProjectJSON))
	if n.Hints != nil && n.hintsDirty {
		_ = n.Hints.Save(ctx, n.CacheFile) // Disposable cache, one write per command.
		n.hintsDirty = false
	}
	return err
}
func (n *Native) Find(ctx context.Context, s memory.Selector, first bool) (memory.LookupResult, error) {
	return n.find(ctx, n.Process, s, first)
}

func (n *Native) find(ctx context.Context, source memory.Source, s memory.Selector, first bool) (memory.LookupResult, error) {
	return n.findPath(ctx, source, s, first, false)
}

func (n *Native) findPath(ctx context.Context, source memory.Source, s memory.Selector, first, nearby bool) (memory.LookupResult, error) {
	if n.TraceDir != "" && n.traceSequence >= observationLimit {
		return memory.LookupResult{}, errors.New("live.channel_observation_budget")
	}
	var found memory.LookupResult
	var err error
	if n.Mailbox != nil {
		found, err = n.Mailbox.Lookup(ctx, s)
		if errors.Is(err, memory.ErrMailboxUnavailable) {
			err = fmt.Errorf("live.channel_mailbox_unavailable: %w", err)
		}
	} else if n.Process != nil {
		return memory.LookupResult{}, errors.New("live.channel_mailbox_required")
	} else if nearby {
		found, err = memory.FindNearby(ctx, source, s, n.Hints)
	} else {
		found, err = memory.Find(ctx, source, s, n.Hints, first)
	}
	n.hintsDirty = n.Hints != nil
	// Coverage/latency persist in host evidence without duplicating large bodies.
	evidence := found
	evidence.Records = nil
	if len(n.Lookups) < observationLimit {
		n.Lookups = append(n.Lookups, evidence)
	}
	if n.TraceDir != "" {
		n.traceSequence++
		trace := lookupTrace{Nonce: hex.EncodeToString(s.Nonce[:]), Runtime: hex.EncodeToString(s.Runtime[:]), Ticket: hex.EncodeToString(s.Ticket[:]), Kind: s.Kind, Matches: len(found.Records), Lookup: evidence, sequence: n.traceSequence}
		if n.Mailbox != nil {
			binding := n.Mailbox.Binding()
			trace.RootBinding = &binding
		}
		n.traces = append(n.traces, trace)
	}
	return found, err
}

// Successful writes leave the pending buffer; a failed write and everything
// after it remain retryable by Close. The total observation count never resets.
func (n *Native) flushTraces(ctx context.Context, write func(context.Context, string, any) error) error {
	completed := 0
	defer func() {
		for i := 0; i < completed; i++ {
			n.traces[i] = lookupTrace{}
		}
		n.traces = n.traces[completed:]
		if len(n.traces) == 0 {
			n.traces = nil
		}
	}()
	for _, trace := range n.traces {
		if err := ctx.Err(); err != nil {
			return errors.Join(ErrTraceFlush, err)
		}
		file := filepath.Join(n.TraceDir, fmt.Sprintf("%03d.json", trace.sequence))
		if err := write(ctx, file, trace); err != nil {
			return fmt.Errorf("%w: %s: %w", ErrTraceFlush, file, err)
		}
		completed++
	}
	return nil
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
			return errors.Join(&BlockedError{Blocker: Blocker{
				Kind: "publication_lock", Installation: n.Parent,
				Condition: "publication_lease_released",
			}}, err)
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
		// Read the exact reservation before releasing admission. Diagnostics
		// must not accidentally describe a later owner's reused physical slot.
		pool, inspectErr := delivery.InspectSlots(ctx, n.Parent, n.Version)
		if inspectErr != nil {
			return inspectErr
		}
		reserved := pool.Files[e.Index-1]
		blocked := &BlockedError{Blocker: Blocker{
			Kind: "slot_reservation", Installation: n.Parent, Slot: e.Index,
			Consumer: reserved.Consumer, Runtime: reserved.Runtime, Nonce: reserved.Nonce,
			Condition: "exact_consumption_or_retirement_evidence",
		}}
		if e.Schema == bridge.SlotSchema {
			if next := delivery.NextFreeSlot(pool, e.Index); next != 0 && (e.Action != "prepare" || next <= bridge.SlotCount-4) {
				return &slotAvailable{Index: next, Blocked: blocked}
			}
			blocked.Blocker.Kind = "slot_pool_full"
			blocked.Blocker.Condition = "forward_slot_available"
		}
		return blocked
	}
	return err
}

// RetryFirstBinding is deliberately limited to a pristine pool containing only
// this non-business bind. Even an unexpected extra wake cannot load business
// code. General input replay needs current-slot evidence and is not allowed.
func (n *Native) RetryFirstBinding(ctx context.Context, e bridge.SlotEnvelope, capability string) (err error) {
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
	// No scan or capture wait while holding the shared publication lease.
	if err = n.unlockPublication(); err != nil {
		return err
	}
	s, err := n.ObserveInput(ctx, e, 0, capability)
	if err != nil {
		return err
	}
	_, err = n.Input(ctx, InputAction{Kind: "invoke", Envelope: e, Observation: &s, Capability: capability})
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
// selected runtime. A public descriptor never authorizes a business script.
func (n *Native) Discover(ctx context.Context, character, realm string) ([]Identity, memory.Coverage, error) {
	found, err := n.Find(ctx, memory.Selector{Kind: bridge.MemoryIdentity}, false)
	if err != nil {
		return nil, found.Coverage, err
	}
	identities := []Identity{}
	seen := map[string]bool{}
	for _, record := range found.Records {
		i, decodeErr := decodeIdentityRecord(record, n.Version)
		if decodeErr != nil {
			continue
		}
		if (character != "" && i.Character != character) || (realm != "" && i.Realm != realm) {
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
