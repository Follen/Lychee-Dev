//go:build windows && amd64

package duplexhost

import (
	"context"
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
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

var writeLanes = []string{"data", "control-bindResume", "control-commit", "control-cancel", "control-close", "control-resultAck", "control-reload", "control-lease"}

// WritersDrained holds all eight independent OS lane leases. It must be called
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
// avoiding recursively acquiring its own bindResume lane. No ordinary publish
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
	wire, err := duplex.EncodeMessage(m)
	if err != nil {
		return out, err
	}
	if err = n.CheckWriteCapability(); err != nil {
		return out, err
	}
	if err = n.checkLifecycleWriteGate(ctx, m.Header.Kind); err != nil {
		return out, err
	}
	lane := "control-" + m.Header.Kind.Lane()
	if m.Header.Kind == duplex.Frame {
		lane = "data"
	}
	if n.repairDrainHeld && m.Header.Kind != duplex.Repair {
		return out, errors.New("live.duplex_drain_publication_invalid")
	}
	if !n.repairDrainHeld {
		lock, e := vault.AcquireLease(ctx, n.scope(), n.resource(lane))
		if e != nil {
			return out, e
		}
		defer func() { returned = errors.Join(returned, lock.Close()) }()
	}
	if n.Guard != nil {
		if err = n.Guard(ctx, m.Header.Kind); err != nil {
			return out, err
		}
	}
	s, err := n.Observe(ctx)
	if err != nil {
		return out, err
	}
	h := m.Header
	if h.Kind == duplex.Bind {
		if e := duplex.ValidateBindTransition(s, m); e != nil {
			return out, e
		}
	}
	if s.Runtime != h.Runtime || s.Arena != h.Arena || s.ActorBinding != h.ActorBinding {
		return out, duplex.ErrIdentity
	}
	if h.Kind != duplex.Bind && (s.Session != h.Session || s.Owner != h.Owner || s.Fence != h.Fence) {
		return out, duplex.ErrIdentity
	}
	if h.Kind == duplex.Frame && s.Request != nil && (s.Request.RequestID != h.RequestID || s.Request.RequestSHA256 != h.RequestSHA256) {
		return out, duplex.ErrBusy
	}
	if h.Kind == duplex.Frame {
		if err = duplex.ValidateFrameTransition(s, m); err != nil {
			return out, err
		}
	}
	if h.Kind == duplex.Frame || h.Kind == duplex.Commit {
		if !s.TransportReady || !s.ActorReady || s.ActorGUID == "" || n.ExpectedActorGUID != "" && s.ActorGUID != n.ExpectedActorGUID {
			return out, duplex.ErrIdentity
		}
	} else if !s.ControlReady {
		return out, duplex.ErrPending
	}
	path := []memory.DuplexPath{{Name: "inbox"}, {Name: "control"}, {Name: h.Kind.Lane()}}
	count := 80 + 256
	if h.Kind == duplex.Frame {
		path = []memory.DuplexPath{{Name: "inbox"}, {Name: "request"}, {Name: "frames"}, {Index: 1}}
		count = 80 + 1024
	}
	array, err := n.Mailbox.ResolveDuplexArray(ctx, path, h.Runtime, h.Arena, count)
	if err != nil {
		return out, err
	}
	// A writer profile must establish independent layout and lifetime evidence.
	// A root recipe or six-number calibration grants no write authority.
	binding := n.Mailbox.Binding()
	if binding.Evidence.RecipeID != memory.LuaMailboxRootRecipeID {
		return out, errors.New("live.duplex_write_profile_unsupported")
	}
	if n.TraceDir == "" {
		return out, errors.New("live.duplex_write_journal_required")
	}
	if err = os.MkdirAll(n.TraceDir, 0700); err != nil {
		return out, errors.Join(duplex.ErrPersistence, err)
	}
	file, err := os.OpenFile(filepath.Join(n.TraceDir, fmt.Sprintf("%s-%d.jsonl", h.MessageID, h.PublicationSeq)), os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
	if err != nil {
		return out, errors.Join(duplex.ErrPersistence, err)
	}
	defer func() {
		if e := file.Close(); e != nil {
			returned = errors.Join(returned, duplex.ErrPersistence, e)
		}
	}()
	intent := struct {
		Message duplex.Message        `json:"message"`
		Root    memory.LuaRootBinding `json:"root"`
		Target  live.ClientWindow     `json:"target"`
	}{m, binding, n.Target}
	enc := json.NewEncoder(file)
	if err = enc.Encode(intent); err != nil {
		return out, errors.Join(duplex.ErrPersistence, err)
	}
	if err = file.Sync(); err != nil {
		return out, errors.Join(duplex.ErrPersistence, err)
	}
	if err = array.Verify(ctx); err != nil {
		return out, err
	}
	writer, err := memory.OpenDuplexWriter(n.Target.Window.ProcessID, n.Target.Window.ProcessStartedAt, n.Target.Window.Executable)
	if err != nil {
		return out, err
	}
	defer func() { returned = errors.Join(returned, writer.Close()) }()
	words := make([]uint32, count)
	for i := 0; i < len(wire); i += 4 {
		var packed [4]byte
		copy(packed[:], wire[i:min(i+4, len(wire))])
		words[i/4] = binary.LittleEndian.Uint32(packed[:])
	}
	writeCount := 0
	write := func(index int, value uint32) error {
		if writeCount%32 == 0 {
			if n.BatchGuard != nil {
				if e := n.BatchGuard(ctx, h.Kind); e != nil {
					return e
				}
			}
			if e := array.Verify(ctx); e != nil {
				return e
			}
		}
		cell := array.Cells[index]
		nbytes, e := writer.WriteDuplexCell(ctx, cell, value, func(c context.Context) error { return n.checkLifecycleWriteGate(c, h.Kind) })
		if nbytes > 0 {
			out.Bytes += uint64(nbytes)
			out.State = duplex.PartialWrite
		}
		fact := struct {
			Index int    `json:"index"`
			Bytes int    `json:"bytes"`
			Error string `json:"error,omitempty"`
		}{Index: index + 1, Bytes: nbytes}
		if e != nil {
			fact.Error = e.Error()
		}
		journalErr := enc.Encode(fact)
		if journalErr != nil {
			journalErr = errors.Join(duplex.ErrPersistence, journalErr)
		}
		writeCount++
		if e != nil || nbytes != 8 || journalErr != nil {
			return errors.Join(e, journalErr, io.ErrShortWrite)
		}
		array.Cells[index].Value = value
		return nil
	}
	// begin low word becomes odd first. No stamp is treated as atomic.
	odd := h.PublicationBegin - 1
	for _, entry := range []struct {
		index int
		value uint32
	}{{74, uint32(odd)}, {75, uint32(odd >> 32)}, {76, uint32(odd)}, {77, uint32(odd >> 32)}} {
		if err = write(entry.index, entry.value); err != nil {
			return out, err
		}
	}
	for i, value := range words {
		if i >= 74 && i <= 77 {
			continue
		}
		if err = write(i, value); err != nil {
			return out, err
		}
	}
	for _, i := range []int{76, 77, 75, 74} {
		if err = write(i, words[i]); err != nil {
			return out, err
		}
	}
	check, err := n.Mailbox.ResolveDuplexArray(ctx, path, h.Runtime, h.Arena, count)
	if err != nil {
		return out, err
	}
	for i, value := range words {
		if check.Cells[i].Value != value {
			return out, errors.New("live.duplex_readback_mismatch")
		}
	}
	if err = file.Sync(); err != nil {
		return out, errors.Join(duplex.ErrPersistence, err)
	}
	out.State = duplex.CompleteWrite
	out.ReadbackVerified = true
	return out, nil
}

func digestBytes(b []byte) string { h := sha256.Sum256(b); return hex.EncodeToString(h[:]) }
