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
	Target                    desktop.WindowIdentity
	Parent, Version, Consumer string
	Guard                     func(context.Context) error
	Process                   *memory.Process
	Mailbox                   mailboxLookup
	Publication               *vault.Lease
	Lookups                   []memory.LookupResult
	TraceDir                  string
	traceSequence             int
	traces                    []lookupTrace
	tracesOmitted             int
	inputWait                 inputSampleWait
	inputAttemptProbe         receiptAttemptProbe
}

type mailboxLookup interface {
	Lookup(context.Context, memory.Selector) (memory.LookupResult, error)
	Binding() memory.LuaRootBinding
}

const observationLimit = 256
const diagnosticFlushBudget = 10 * time.Second

type lookupTrace struct {
	Nonce, Runtime, Ticket string
	Kind                   bridge.MemoryKind
	Matches                int
	Lookup                 memory.LookupResult
	RootBinding            *memory.LuaRootBinding `json:"rootBinding,omitempty"`
	Error                  string                 `json:"error,omitempty"`
	sequence               int
}

func (n *Native) Observe(ctx context.Context, q ObservationQuery) (Observation, error) {
	observation, err := ObserveRecords(ctx, n, q)
	if n.Mailbox == nil {
		return observation, err
	}
	return n.inputAttemptProbe.observe(ctx, n, q, observation, err, uptimeMillis())
}

func OpenNative(ctx context.Context, target desktop.WindowIdentity, parent, version, _ string, _ bool) (*Native, error) {
	process, err := memory.Open(target.ProcessID, target.ProcessStartedAt, target.Executable)
	if err != nil {
		return nil, err
	}
	n := &Native{Target: target, Parent: parent, Version: version, Process: process, Consumer: fmt.Sprintf("%d/%d", target.ProcessID, target.ProcessStartedAt)}
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
	// Never retain an input/publication lease while writing diagnostics. This
	// final bounded flush also runs after the command context was cancelled.
	ctx, cancel := context.WithTimeout(context.Background(), diagnosticFlushBudget)
	defer cancel()
	err = errors.Join(err, n.flushTraces(ctx, writeProjectJSON))

	return err
}
func (n *Native) Find(ctx context.Context, s memory.Selector, _ bool) (memory.LookupResult, error) {
	if n.Mailbox == nil {
		return memory.LookupResult{}, errors.New("live.channel_mailbox_required")
	}
	found, err := n.Mailbox.Lookup(ctx, s)
	if errors.Is(err, memory.ErrMailboxUnavailable) {
		err = fmt.Errorf("live.channel_mailbox_unavailable: %w", err)
	}
	// Coverage/latency persist in host evidence without duplicating large bodies.
	evidence := found
	evidence.Records = nil
	if len(n.Lookups) < observationLimit {
		n.Lookups = append(n.Lookups, evidence)
	}
	if n.TraceDir != "" && n.traceSequence >= observationLimit {
		n.tracesOmitted++
	} else if n.TraceDir != "" {
		n.traceSequence++
		trace := lookupTrace{Nonce: hex.EncodeToString(s.Nonce[:]), Runtime: hex.EncodeToString(s.Runtime[:]), Ticket: hex.EncodeToString(s.Ticket[:]), Kind: s.Kind, Matches: len(found.Records), Lookup: evidence, sequence: n.traceSequence}
		if err != nil {
			trace.Error = err.Error()
		}
		// The binding is immutable for this Native invocation. Its sparse scan
		// ledger can be large; retain it once, in the first trace of this directory.
		if n.Mailbox != nil && n.traceSequence == 1 {
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
	if n.tracesOmitted > 0 {
		summary := struct {
			Observations int  `json:"observations"`
			Retained     int  `json:"retained"`
			Omitted      int  `json:"omitted"`
			Truncated    bool `json:"truncated"`
		}{n.traceSequence + n.tracesOmitted, n.traceSequence, n.tracesOmitted, true}
		if err := write(ctx, filepath.Join(n.TraceDir, "summary.json"), summary); err != nil {
			return errors.Join(ErrTraceFlush, err)
		}
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
			// A present, incompatible descriptor is not an absent installation.
			// Do not turn a protocol error into blind first-install reload input.
			return nil, found.Coverage, decodeErr
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
