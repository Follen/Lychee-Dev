//go:build windows && amd64

package duplexhost

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"reflect"

	"github.com/follenfang/lycheedev/internal/buildinfo"
	"github.com/follenfang/lycheedev/internal/live"
	"github.com/follenfang/lycheedev/internal/live/duplex"
	"github.com/follenfang/lycheedev/internal/live/memory"
	"github.com/follenfang/lycheedev/internal/vault"
)

type duplexMailbox interface {
	ReadDuplexString(context.Context, []memory.DuplexPath, int) ([]byte, error)
	ResolveDuplexArray(context.Context, []memory.DuplexPath, string, string, int) (*memory.DuplexArray, error)
	Binding() memory.LuaRootBinding
}
type Native struct {
	Target            live.ClientWindow
	Process           *memory.Process
	Mailbox           duplexMailbox
	Reload            *memory.ReloadBinding
	reloadErr         error
	Guard             func(context.Context, duplex.Kind) error
	BatchGuard        func(context.Context, duplex.Kind) error
	ExpectedActorGUID string
	TraceDir          string
	observe           func(context.Context) (duplex.Sendbox, error)
	repairDrainHeld   bool
}

func OpenNative(ctx context.Context, target live.ClientWindow) (*Native, error) {
	p, err := memory.Open(target.Window.ProcessID, target.Window.ProcessStartedAt, target.Window.Executable)
	if err != nil {
		return nil, err
	}
	m, err := p.MainModule(ctx)
	if err != nil {
		p.Close()
		return nil, err
	}
	reload, reloadErr := memory.ResolveReloadState(ctx, m.Base, m.Size, m.ExecutableSHA256, target.Client.FullBuild, target.Client.Product, m.LuaImageLayout(), func(c context.Context, at uint64, b []byte) (int, error) { return p.ReadModule(c, m, at, b) })
	if reload != nil {
		observation, e := reload.Observe(ctx)
		if e != nil {
			reloadErr = e
		}
		if e == nil && errors.Is(observation.CheckWriteGate(), memory.ErrReloadActive) {
			p.Close()
			return nil, observation.CheckWriteGate()
		}
	}
	r, err := memory.OpenLuaMailbox(ctx, p, m.Base, m.Size, m.ExecutableSHA256, buildinfo.Version, m.LuaImageLayout(), func(c context.Context, at uint64, b []byte) (int, error) { return p.ReadModule(c, m, at, b) })
	if err != nil {
		p.Close()
		return nil, err
	}
	return &Native{Target: target, Process: p, Mailbox: r, Reload: reload, reloadErr: reloadErr}, nil
}

func (n *Native) scope() string {
	return filepath.Join(n.Target.Client.Directory, "Interface", "AddOns", ".lycheedev-duplex-writers")
}
func (n *Native) resource(lane string) string {
	return fmt.Sprintf("%d/%d/%s", n.Target.Window.ProcessID, n.Target.Window.ProcessStartedAt, lane)
}
func (n *Native) Close(context.Context) error {
	if n.Process == nil {
		return nil
	}
	err := n.Process.Close()
	n.Process = nil
	return err
}

func (n *Native) Observe(ctx context.Context) (duplex.Sendbox, error) {
	if n.observe != nil {
		return n.observe(ctx)
	}
	if err := live.ConfirmClientWindow(ctx, n.Target); err != nil {
		return duplex.Sendbox{}, err
	}
	b, err := n.Mailbox.ReadDuplexString(ctx, []memory.DuplexPath{{Name: "sendbox"}, {Name: "status"}}, duplex.MaxSendboxBytes+44)
	if err != nil {
		return duplex.Sendbox{}, fmt.Errorf("live.duplex_runtime_unavailable: %w", err)
	}
	s, err := duplex.DecodeSendbox(b)
	if err != nil {
		return s, err
	}
	if s.Build != n.Target.Client.FullBuild || s.Product != n.Target.Client.Product || s.Release != buildinfo.Version {
		return s, errors.New("live.duplex_runtime_build_or_release_mismatch")
	}
	return s, nil
}

func (n *Native) WriteCapability() memory.DuplexWriterProfile {
	return memory.DuplexWriteCapability(n.Mailbox.Binding().ExecutableSHA256, n.Target.Client.FullBuild, n.Target.Client.Product)
}
func (n *Native) CheckWriteCapability() error {
	if !n.WriteCapability().Eligible {
		return errors.New("live.duplex_writer_profile_unverified")
	}
	return nil
}

func (n *Native) ObserveReload(ctx context.Context) (memory.ReloadObservation, error) {
	if n.Reload == nil {
		return memory.ReloadObservation{State: "unknown", WorldState: "unknown", Reason: "reload recipe unavailable"}, errors.Join(memory.ErrReloadUnknown, n.reloadErr)
	}
	return n.Reload.Observe(ctx)
}

func (n *Native) CheckReloadWriteGate(ctx context.Context) error {
	o, err := n.ObserveReload(ctx)
	if err != nil {
		return err
	}
	return o.CheckWriteGate()
}

// Leaving the world closes business admission, not cancellation or cleanup.
// Every kind still needs a fresh reload gate; none establishes a lifetime pin.
func (n *Native) checkLifecycleWriteGate(ctx context.Context, kind duplex.Kind) error {
	o, err := n.ObserveReload(ctx)
	if err != nil {
		return err
	}
	if kind == duplex.Bind || kind == duplex.Frame || kind == duplex.Commit {
		return o.CheckBusinessWriteGate()
	}
	return o.CheckWriteGate()
}

func (n *Native) ReadResult(ctx context.Context, m duplex.ResultManifest) ([]byte, error) {
	if m.Bytes > duplex.MaxResultBytes || m.Pages < 1 || m.Pages > 32 || len(m.PageSHA256) != int(m.Pages) {
		return nil, errors.New("live.duplex_result_shape")
	}
	before, err := n.Observe(ctx)
	if err != nil {
		return nil, err
	}
	if before.Terminal == nil || !reflect.DeepEqual(*before.Terminal, m) {
		return nil, duplex.ErrIdentity
	}
	out := make([]byte, 0, m.Bytes)
	for i := uint32(1); i <= m.Pages; i++ {
		b, e := n.Mailbox.ReadDuplexString(ctx, []memory.DuplexPath{{Name: "sendbox"}, {Name: "resultPages"}, {Index: int(i)}}, 16384)
		if e != nil {
			return nil, e
		}
		if len(out)+len(b) > int(m.Bytes) {
			return nil, errors.New("live.duplex_result_overflow")
		}
		expected := 16384
		if i == m.Pages {
			expected = int(m.Bytes) - (int(m.Pages)-1)*16384
		}
		if len(b) != expected || digestBytes(b) != m.PageSHA256[i-1] {
			return nil, errors.New("live.duplex_result_page_mismatch")
		}
		out = append(out, b...)
	}
	after, err := n.Observe(ctx)
	if err != nil {
		return nil, err
	}
	if after.Identity != before.Identity || after.Terminal == nil || !reflect.DeepEqual(*after.Terminal, m) {
		return nil, duplex.ErrIdentity
	}
	return out, duplex.ValidateResult(m, out)
}

var writeLanes = []string{"command", "stop"}

// WritersDrained holds the independent command and stop row leases. It must be called
// before repair permission, never inferred from an addon status boolean.
func (n *Native) WritersDrained(ctx context.Context) (func(), error) {
	var held []*vault.Lease
	release := func() {
		for i := len(held) - 1; i >= 0; i-- {
			_ = held[i].Close()
		}
	}
	for _, lane := range writeLanes {
		lease, e := vault.TryAcquireLease(ctx, n.scope(), n.resource(lane))
		if e != nil {
			release()
			return nil, e
		}
		held = append(held, lease)
	}
	return release, nil
}

// Repair holds the drain proof across the repair publication, while explicitly
// avoiding recursively acquiring its own stop row lease. No ordinary publish
// may borrow these leases.
func (n *Native) repair(ctx context.Context, c *duplex.Coordinator) (duplex.State, error) {
	if e := n.CheckWriteCapability(); e != nil {
		return duplex.State{}, e
	}
	release, e := n.WritersDrained(ctx)
	if e != nil {
		return duplex.State{}, e
	}
	defer release()
	n.repairDrainHeld = true
	defer func() { n.repairDrainHeld = false }()
	return c.Repair(ctx, true)
}

func (n *Native) Publish(ctx context.Context, m duplex.Message) (out duplex.WriteOutcome, returned error) {
	out.State = duplex.NoWrite
	if _, ok := ctx.Deadline(); !ok {
		return out, errors.New("live.duplex_deadline_required")
	}
	if m.Header.Kind != duplex.Frame && m.Header.Kind != duplex.Cancel && m.Header.Kind != duplex.Close && m.Header.Kind != duplex.Repair && m.Header.Kind != duplex.Reload && m.Header.Kind != duplex.Lease {
		return out, errors.New("live.mailbox_control_unavailable")
	}
	if _, err := duplex.EncodeMessage(m); err != nil {
		return out, err
	}
	if err := n.CheckWriteCapability(); err != nil {
		return out, err
	}
	lane := "stop"
	if m.Header.Kind == duplex.Frame {
		lane = "command"
	}
	if n.repairDrainHeld && m.Header.Kind != duplex.Repair {
		return out, errors.New("live.duplex_drain_publication_invalid")
	}
	if !n.repairDrainHeld {
		lease, e := vault.AcquireLease(ctx, n.scope(), n.resource(lane))
		if e != nil {
			return out, e
		}
		defer func() { returned = errors.Join(returned, lease.Close()) }()
	}
	actorGUID := n.ExpectedActorGUID
	guard := func(c context.Context) error {
		if err := n.checkLifecycleWriteGate(c, m.Header.Kind); err != nil {
			return err
		}
		if n.Guard != nil {
			if err := n.Guard(c, m.Header.Kind); err != nil {
				return err
			}
		}
		if n.BatchGuard != nil {
			if err := n.BatchGuard(c, m.Header.Kind); err != nil {
				return err
			}
		}
		s, err := n.Observe(c)
		if err != nil {
			return err
		}
		h := m.Header
		if s.Runtime != h.Runtime || s.Arena != h.Arena || s.ActorBinding != h.ActorBinding {
			return duplex.ErrIdentity
		}
		if actorGUID == "" {
			actorGUID = s.ActorGUID
		}
		if s.ActorGUID == "" || s.ActorGUID != actorGUID {
			return duplex.ErrIdentity
		}
		if h.Kind == duplex.Frame {
			if err = duplex.ValidateFrameTransition(s, m); err != nil {
				return err
			}
		} else {
			matched := s.Session == h.Session && s.Owner == h.Owner && s.Fence == h.Fence
			if !matched {
				candidate, e := duplex.NextIdentity(s, h.Owner, h.Session)
				matched = e == nil && candidate == (duplex.Identity{Runtime: h.Runtime, Arena: h.Arena, Session: h.Session, Owner: h.Owner, ActorBinding: h.ActorBinding, Fence: h.Fence}) && (s.ReadyChallenge == h.Challenge || h.Kind == duplex.Repair && s.Repair != nil && s.Repair.Challenge == h.Challenge)
			}
			if !matched {
				return duplex.ErrIdentity
			}
			if !s.ControlReady {
				return duplex.ErrPending
			}
		}
		return nil
	}
	if err := guard(ctx); err != nil {
		return out, err
	}
	binding := n.Mailbox.Binding()
	if binding.Evidence.RecipeID != memory.LuaMailboxRootRecipeID {
		return out, errors.New("live.duplex_write_profile_unsupported")
	}
	return n.publishJournaled(ctx, m, binding, actorGUID, "direct", memory.PublishDirectDuplexRow)
}

type rowPublisher func(context.Context, memory.StoppedPublicationRequest) (duplex.WriteOutcome, []memory.DuplexWriteRange, memory.StoppedObservation, error)

// publishJournaled syncs the exact intent before the selected typed writer
// runs. Neither a complete WPM nor its readback proves addon execution.
func (n *Native) publishJournaled(ctx context.Context, m duplex.Message, binding memory.LuaRootBinding, actorGUID, mode string, publish rowPublisher) (out duplex.WriteOutcome, returned error) {
	out.State = duplex.NoWrite
	if mode != "direct" && mode != "stopped" {
		return out, errors.New("live.duplex_write_mode_invalid")
	}
	if n.TraceDir == "" {
		return out, errors.New("live.duplex_write_journal_required")
	}
	if err := os.MkdirAll(n.TraceDir, 0700); err != nil {
		return out, errors.Join(duplex.ErrPersistence, err)
	}
	file, err := os.OpenFile(filepath.Join(n.TraceDir, fmt.Sprintf("%s-%d.jsonl", m.Header.MessageID, m.Header.PublicationSeq)), os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
	if err != nil {
		return out, errors.Join(duplex.ErrPersistence, err)
	}
	defer func() {
		if e := file.Close(); e != nil {
			returned = errors.Join(returned, duplex.ErrPersistence, e)
		}
	}()
	enc := json.NewEncoder(file)
	// Command bytes are stored once in the coordinator journal. This immutable
	// reference and exact header are synced before starting the native helper.
	header := m.Header
	intent := struct {
		Mode          string                `json:"mode"`
		Header        duplex.Header         `json:"header"`
		PayloadSHA256 string                `json:"payloadSHA256"`
		PayloadBytes  int                   `json:"payloadBytes"`
		Root          memory.LuaRootBinding `json:"root"`
		ActorGUID     string                `json:"actorGUID"`
		Target        live.ClientWindow     `json:"target"`
	}{mode, header, digestBytes(m.Payload), len(m.Payload), binding, actorGUID, n.Target}
	if err = enc.Encode(intent); err != nil {
		return out, errors.Join(duplex.ErrPersistence, err)
	}
	if err = file.Sync(); err != nil {
		return out, errors.Join(duplex.ErrPersistence, err)
	}
	request := memory.StoppedPublicationRequest{
		Target:           memory.ProcessIdentity{PID: n.Target.Window.ProcessID, Created: n.Target.Window.ProcessStartedAt, Image: n.Target.Window.Executable},
		ExecutableSHA256: binding.ExecutableSHA256, Build: n.Target.Client.FullBuild, Product: n.Target.Client.Product, Release: buildinfo.Version, ActorGUID: actorGUID, Message: m,
	}
	out, facts, stop, writeErr := publish(ctx, request)
	// No disk wait occurs inside the native write critical section.
	fact := struct {
		Mode    string                    `json:"mode"`
		Outcome duplex.WriteOutcome       `json:"outcome"`
		Ranges  []memory.DuplexWriteRange `json:"ranges"`
		Stop    memory.StoppedObservation `json:"stop"`
		Error   string                    `json:"error,omitempty"`
	}{Mode: mode, Outcome: out, Ranges: facts, Stop: stop}
	if writeErr != nil {
		fact.Error = writeErr.Error()
	}
	if err = enc.Encode(fact); err == nil {
		err = file.Sync()
	}
	if err != nil {
		return out, errors.Join(writeErr, duplex.ErrPersistence, err)
	}
	return out, writeErr
}

func digestBytes(b []byte) string { h := sha256.Sum256(b); return hex.EncodeToString(h[:]) }
