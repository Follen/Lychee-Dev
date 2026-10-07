//go:build windows && amd64

package channel

import (
	"context"
	"crypto/sha256"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/buildinfo"
	"github.com/follenfang/lycheedev/internal/delivery"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/live"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/records"
	"github.com/follenfang/lycheedev/internal/vault"
)

var ErrActivationRequired = errors.New("live.channel_activation_required")
var ErrClosed = errors.New("live.channel_closed")

type TargetRequest struct {
	Installation                     string
	PID                              uint32
	Character, Realm, Build, Product string
}

// ProjectResult is presentation, not a second persisted state machine. Raw
// evidence remains in the connection log and immutable result-byte artifacts.
type ProjectResult struct {
	Continuation   Continuation             `json:"continuation"`
	ProcessEnd     string                   `json:"processEnd,omitempty"`
	RuntimeEnd     *RuntimeReplacementProof `json:"runtimeEnd,omitempty"`
	Input          *InputAttempt            `json:"input,omitempty"`
	Waiting        string                   `json:"waiting,omitempty"`
	Session        string                   `json:"session"`
	Operation      string                   `json:"operation,omitempty"`
	OperationState string                   `json:"operationState,omitempty"`
	Scans          string                   `json:"scans,omitempty"`
	Identity       Identity                 `json:"identity"`
	Bound          bool                     `json:"bound"`
	Closed         bool                     `json:"closed"`
	Stage          string                   `json:"stage"`
	Complete       bool                     `json:"complete"`
	ReportState    string                   `json:"reportState"`
	Cleanup        string                   `json:"cleanup"`
	CleanupMethod  string                   `json:"cleanupMethod,omitempty"`
	Report         json.RawMessage          `json:"report,omitempty"`
	Journal        string                   `json:"journal"`
	Reload         *ReloadAttempt           `json:"reload,omitempty"`
	ReportOrigin   *Identity                `json:"reportOrigin,omitempty"`
}

type projectTarget struct {
	Schema string              `json:"schema"`
	Target live.ClientWindow   `json:"target"`
	Owner  journal.WindowOwner `json:"owner"`
}

type Project struct {
	Root string
}

func OpenProject(directory string) (*Project, error) {
	if directory == "" {
		var err error
		directory, err = os.Getwd()
		if err != nil {
			return nil, err
		}
	}
	root, err := filepath.Abs(directory)
	if err != nil {
		return nil, err
	}
	root, err = filepath.EvalSymlinks(root)
	if err != nil {
		return nil, err
	}
	info, err := os.Stat(root)
	if err != nil {
		return nil, err
	}
	if !info.IsDir() {
		return nil, errors.New("live.channel_project_invalid")
	}
	return &Project{Root: root}, nil
}
func (p *Project) path(parts ...string) string {
	return filepath.Join(append([]string{p.Root, ".lycheedev", "live"}, parts...)...)
}
func (p *Project) workspaceID() string {
	h := sha256.Sum256([]byte(strings.ToLower(p.Root)))
	return fmt.Sprintf("%x", h[:16])
}
func connectionID(id string) error {
	if !strings.HasPrefix(id, "CON-") {
		return errors.New("live.channel_connection_id_required")
	}
	_, err := tokenBytes(strings.TrimPrefix(id, "CON-"))
	return err
}
func (p *Project) log(id string) string { return p.path("connections", id+".jsonl") }
func writeProjectJSON(ctx context.Context, path string, value any) error {
	b, err := json.Marshal(value)
	if err != nil {
		return err
	}
	return vault.ReplaceFile(ctx, path, b)
}
func readProjectJSON(path string, value any, limit int64) error {
	info, err := os.Lstat(path)
	if err != nil {
		return err
	}
	if !info.Mode().IsRegular() || info.Size() > limit {
		return errors.New("live.channel_record_invalid")
	}
	f, err := os.Open(path)
	if err != nil {
		return err
	}
	defer f.Close()
	b, err := io.ReadAll(io.LimitReader(f, limit+1))
	if err != nil {
		return err
	}
	if int64(len(b)) > limit {
		return errors.New("live.channel_record_limit")
	}
	return json.Unmarshal(b, value)
}
func (p *Project) metadata(id string) (projectTarget, error) {
	var meta projectTarget
	if err := connectionID(id); err != nil {
		return meta, err
	}
	err := readProjectJSON(p.path("connections", id+".target.json"), &meta, 16384)
	if err != nil {
		return meta, err
	}
	if meta.Schema != "lycheedev.channel-target.v1" || meta.Owner.OperationID != id || meta.Owner.WorkspaceID != p.workspaceID() {
		return meta, errors.New("live.channel_project_owner_mismatch")
	}
	return meta, nil
}
func present(d *Driver) ProjectResult {
	s := d.State
	r := ProjectResult{Session: s.ID, Identity: s.Identity, Bound: s.Bound, Closed: s.Closed, Stage: "connecting", Complete: s.Bound || s.Closed, ReportState: "unavailable", Cleanup: "none", Journal: d.Log}
	r.Reload = s.Reload
	r.ProcessEnd = s.ProcessEnd
	r.RuntimeEnd = s.RuntimeEnd
	r.Input = s.Input
	r.Waiting = d.Waiting
	r.Continuation = d.continuation()
	if native, ok := d.Backend.(*Native); ok {
		r.Scans = native.TraceDir
	}
	if s.Bound {
		r.Stage = "connected"
	}
	if s.Closed {
		r.Stage = "closed"
	}
	if op := s.Operation; op != nil {
		r.CleanupMethod = op.CleanupMethod
		r.OperationState = op.Stage
		r.ReportOrigin = op.Origin
		r.Operation = op.ID
		r.Stage = op.Stage
		r.Complete = op.Stage == "complete"
		r.Cleanup = "pending"
		if op.Stage == "result_verified" || op.Stage == "release_ready" || op.Stage == "complete" {
			r.ReportState = "verified"
			r.Report = append(json.RawMessage(nil), op.Result...)
		}
		if op.Stage == "complete" {
			r.Cleanup = "complete"
		}
		if op.Stage == "cancelled" {
			r.Cleanup = "complete"
			r.CleanupMethod = "not_executed"
		}
		if op.Stage == "execution_unknown" {
			r.Cleanup = "runtime_lost"
		}
	}
	if s.Closed {
		r.Stage = "closed"
		// Closing ownership cannot turn an interrupted unknown business result
		// into a completed operation. Closed already expresses connection success.
		r.Complete = s.Operation == nil || s.Operation.Stage == "complete"
	}
	if s.Reload != nil && s.Reload.Phase != "complete" {
		r.Stage = "reload_" + s.Reload.Phase
		r.Complete = false
	}
	if s.Transaction != nil || s.Closing || s.Recovery != nil && s.Recovery.Phase != "complete" {
		r.Complete = false
	}
	if s.Closing || s.Transaction != nil && s.Transaction.Envelope.Action == "unbind" {
		r.Stage = "closing"
	}
	return r
}
func (p *Project) Status(id string) (ProjectResult, error) {
	if _, err := p.metadata(id); err != nil {
		return ProjectResult{}, err
	}
	d, err := Load(p.log(id), nil)
	if err != nil {
		if errors.Is(err, ErrJournalMissing) {
			return p.activationStatus(id)
		}
		return ProjectResult{}, err
	}
	return present(d), nil
}

// Main-addon and target identity are stable under the connection claim.
// Shared slot inspection belongs to Native's publication lease, never here:
// another instance may be atomically replacing its authorized payload.
func deploymentGuard(ctx context.Context, target live.ClientWindow) error {
	if err := live.ConfirmClientWindow(ctx, target); err != nil {
		return err
	}
	client, err := records.InspectClientInstallation(ctx, target.Client.Directory)
	if err != nil {
		return err
	}
	status, err := delivery.InspectInstallation(ctx, delivery.AddonDirectory(target.Client.Directory), "addon")
	if err != nil {
		return err
	}
	observed := target
	observed.Client = client
	if !sameConnectionTarget(target, observed) || status.State != "managed" || status.Receipt == nil || status.Receipt.Version != buildinfo.Version {
		return errors.New("live.channel_clean_current_addon_required")
	}
	return nil
}
func (p *Project) native(ctx context.Context, target live.ClientWindow, cache bool) (*Native, error) {
	key := fmt.Sprintf("%d-%d.json", target.Window.ProcessID, target.Window.ProcessStartedAt)
	id, err := token()
	if err != nil {
		return nil, err
	}
	n, err := OpenNative(ctx, target.Window, filepath.Join(target.Client.Directory, "Interface", "AddOns"), buildinfo.Version, p.path("cache", key), cache)
	if err != nil {
		return nil, err
	}
	n.TraceDir = p.path("scans", id)
	return n, nil
}
func claimGuard(target live.ClientWindow, parent string, owner journal.WindowOwner) func(context.Context) error {
	return func(ctx context.Context) error {
		if err := deploymentGuard(ctx, target); err != nil {
			return err
		}
		actual, busy, err := journal.InspectWindowOwner(ctx, parent, owner.Resource)
		if err != nil {
			return err
		}
		if !busy || actual != owner {
			return errors.New("live.channel_ownership_changed")
		}
		return nil
	}
}

func (p *Project) Connect(ctx context.Context, request TargetRequest, cache bool) (result ProjectResult, err error) {
	if _, ok := ctx.Deadline(); !ok {
		return ProjectResult{}, errors.New("live.channel_deadline_required")
	}
	target, err := live.ResolveClientWindow(ctx, request.Installation, request.PID)
	if err != nil {
		return ProjectResult{}, err
	}
	if request.Build != "" && request.Build != target.Client.FullBuild || request.Product != "" && request.Product != target.Client.Product {
		return ProjectResult{}, errors.New("live.channel_target_mismatch")
	}
	resource := fmt.Sprintf("window/%d/%d/%d", target.Window.ProcessID, target.Window.ProcessStartedAt, target.Window.Handle)
	parent := filepath.Join(target.Client.Directory, "Interface", "AddOns")
	if owner, busy, e := journal.InspectWindowOwner(ctx, parent, resource); e != nil {
		return ProjectResult{}, e
	} else if busy {
		if owner.WorkspaceID != p.workspaceID() {
			return ProjectResult{}, &journal.WindowOccupied{Owner: owner, Foreign: true}
		}
		meta, e := p.metadata(owner.OperationID)
		if e != nil {
			return ProjectResult{}, e
		}
		if !sameConnectionTarget(meta.Target, target) || meta.Owner != owner {
			return ProjectResult{}, errors.New("live.channel_ownership_changed")
		}
		status, e := p.Status(owner.OperationID)
		if e != nil {
			return status, e
		}
		if status.Identity.Runtime != "" && (request.Character != "" && request.Character != status.Identity.Character || request.Realm != "" && request.Realm != status.Identity.Realm) {
			return status, errors.New("live.channel_target_mismatch")
		}
		return p.Resume(ctx, owner.OperationID, cache)
	}
	if err = deploymentGuard(ctx, target); err != nil {
		return ProjectResult{}, err
	}
	native, err := p.native(ctx, target, cache)
	if err != nil {
		return ProjectResult{}, err
	}
	defer func() { err = errors.Join(err, native.Close()) }()
	candidates, coverage, err := native.Discover(ctx, request.Character, request.Realm)
	if err != nil {
		return ProjectResult{}, err
	}
	if len(candidates) == 0 {
		if !coverage.Complete || coverage.Truncated {
			return ProjectResult{Scans: native.TraceDir}, errors.New("live.channel_discovery_incomplete")
		}
		key, e := token()
		if e != nil {
			return ProjectResult{}, e
		}
		return p.activateTarget(ctx, target, request, "connect-"+key, "", cache)
	}
	identity := candidates[0]
	if identity.Slots != bridge.SlotCount {
		key, e := token()
		if e != nil {
			return ProjectResult{}, e
		}
		return p.activateTarget(ctx, target, request, "connect-upgrade-"+key, "", cache)
	}
	if identity.Inventory != nil && *identity.Inventory != identity.Slots {
		return ProjectResult{}, errors.New("live.channel_slot_inventory_incomplete")
	}
	if identity.Build != target.Client.FullBuild || identity.Product != target.Client.Product {
		return ProjectResult{}, errors.New("live.channel_runtime_build_mismatch")
	}
	d, err := New("", native, identity)
	if err != nil {
		return ProjectResult{}, err
	}
	d.Log = p.log(d.State.ID)
	d.ResultDir = p.path("results")
	digest := sha256.Sum256([]byte(resource + "/" + d.State.ID))
	owner := journal.WindowOwner{Schema: "lycheedev.window-owner.v1", WorkspaceID: p.workspaceID(), Resource: resource, OperationID: d.State.ID, IntentSHA256: fmt.Sprintf("%x", digest)}
	meta := projectTarget{Schema: "lycheedev.channel-target.v1", Target: target, Owner: owner}
	err = journal.BeginConnectionWindow(ctx, native.Parent, owner, func() error {
		if e := writeProjectJSON(ctx, p.path("connections", d.State.ID+".target.json"), meta); e != nil {
			return e
		}
		return d.Save(ctx, "connection_created")
	})
	if err != nil {
		return present(d), err
	}
	lease, err := journal.LockBootstrapWindow(ctx, native.Parent, owner)
	if err != nil {
		return present(d), err
	}
	defer func() { err = errors.Join(err, lease.Close()) }()
	native.Guard = claimGuard(target, native.Parent, owner)
	err = d.Continue(ctx)
	return present(d), err
}

func (p *Project) drive(ctx context.Context, id string, cache bool, action func(*Driver) error) (result ProjectResult, err error) {
	meta, err := p.metadata(id)
	if err != nil {
		return ProjectResult{}, err
	}
	d, err := Load(p.log(id), nil)
	if err != nil {
		return ProjectResult{}, err
	}
	if d.State.Closed {
		return present(d), ErrClosed
	}
	native, err := p.native(ctx, meta.Target, cache)
	if err != nil {
		return present(d), err
	}
	defer func() { err = errors.Join(err, native.Close()) }()
	lease, err := journal.LockBootstrapWindow(ctx, native.Parent, meta.Owner)
	if err != nil {
		if errors.Is(err, journal.ErrBusy) {
			b := Blocker{Kind: "active_driver", Consumer: meta.Owner.OperationID, Installation: native.Parent, Condition: "active_driver_released"}
			r := present(d)
			r.Continuation.Kind = "wait_active_driver"
			r.Continuation.Blocker = &b
			return r, errors.Join(ErrPending, err)
		}
		return present(d), err
	}
	defer func() {
		if lease != nil {
			err = errors.Join(err, lease.Close())
		}
	}()
	// State read before admission may belong to another driver's in-flight turn.
	// Reload under the lease before deciding any transition or issuing input.
	d, err = Load(p.log(id), nil)
	if err != nil {
		return ProjectResult{}, err
	}
	if d.State.Closed {
		return present(d), ErrClosed
	}
	native.Guard = claimGuard(meta.Target, native.Parent, meta.Owner)
	if err = native.Guard(ctx); err != nil {
		return present(d), err
	}
	d.Backend = native
	err = action(d)
	if err == nil && d.State.Closed {
		if err = lease.Close(); err != nil {
			return present(d), err
		}
		lease = nil
		err = journal.RetireConnectionWindow(ctx, native.Parent, meta.Owner)
	}
	return present(d), err
}
func (p *Project) Resume(ctx context.Context, id string, cache bool) (ProjectResult, error) {
	if err := p.recoverTail(ctx, id); err != nil {
		return ProjectResult{}, err
	}
	state, err := p.Status(id)
	if err != nil {
		return state, err
	}
	if state.Closed {
		return p.retire(ctx, id, state)
	}
	d, err := Load(p.log(id), nil)
	if err != nil {
		if errors.Is(err, ErrJournalMissing) {
			return p.resumeActivation(ctx, id, cache)
		}
		return state, err
	}
	if d.State.RuntimeEnd != nil || d.State.Closing {
		return p.drive(ctx, id, cache, func(d *Driver) error { return d.continueOrRetire(ctx, d.Backend.(*Native), false) })
	}
	if d.State.Reload != nil && d.State.Reload.Phase != "complete" {
		return p.drive(ctx, id, cache, func(d *Driver) error { return d.continueOrRetire(ctx, d.Backend.(*Native), false) })
	}
	if d.State.Transaction == nil && state.Operation != "" && state.Complete {
		return state, nil
	}
	if d.State.Transaction != nil && d.State.Transaction.Envelope.Action == "unbind" {
		return p.Disconnect(ctx, id, cache)
	}
	return p.drive(ctx, id, cache, func(d *Driver) error { return d.continueOrRetire(ctx, d.Backend.(*Native), false) })
}
func (p *Project) Execute(ctx context.Context, id, request, code string, budget int, policy string, cache bool) (ProjectResult, error) {
	if err := p.recoverTail(ctx, id); err != nil {
		return ProjectResult{}, err
	}
	if _, err := p.metadata(id); err != nil {
		return ProjectResult{}, err
	}
	prior, err := requestState(p.log(id), request)
	if err != nil {
		return ProjectResult{}, err
	}
	if err = checkRequest(prior, code, budget, policy); err != nil {
		return ProjectResult{}, err
	}
	if prior != nil && prior.Operation.Stage == "complete" {
		return present(&Driver{State: *prior, Log: p.log(id)}), nil
	}
	var historic *State
	result, err := p.drive(ctx, id, cache, func(d *Driver) error {
		// Recheck after the driver lease: another CLI may have just completed it.
		prior, err := requestState(p.log(id), request)
		if err != nil {
			return err
		}
		if err = checkRequest(prior, code, budget, policy); err != nil {
			return err
		}
		if prior != nil {
			if prior.Operation.Stage == "complete" {
				historic = prior
				return nil
			}
			if d.State.Operation == nil || d.State.Operation.ID != prior.Operation.ID {
				return errors.New("live.channel_request_history_invalid")
			}
			return d.Continue(ctx)
		}
		if err := d.PrepareRequest(ctx, request, code, budget, policy); err != nil {
			return err
		}
		return d.Continue(ctx)
	})
	if historic != nil {
		result = present(&Driver{State: *historic, Log: p.log(id)})
	}
	return result, err
}

func (p *Project) recoverTail(ctx context.Context, id string) error {
	meta, err := p.metadata(id)
	if err != nil {
		return err
	}
	_, err = journal.ReadMemoryLog(p.log(id))
	if !errors.Is(err, journal.ErrMemoryTornTail) {
		return err
	}
	parent := filepath.Join(meta.Target.Client.Directory, "Interface", "AddOns")
	lease, err := journal.LockBootstrapWindow(ctx, parent, meta.Owner)
	if err != nil {
		return err
	}
	defer lease.Close()
	return journal.RecoverMemoryTail(ctx, p.log(id))
}

func (p *Project) retire(ctx context.Context, id string, result ProjectResult) (ProjectResult, error) {
	meta, err := p.metadata(id)
	if err != nil {
		return result, err
	}
	parent := filepath.Join(meta.Target.Client.Directory, "Interface", "AddOns")
	if owner, busy, e := journal.InspectWindowOwner(ctx, parent, meta.Owner.Resource); e != nil {
		return result, e
	} else if busy && owner != meta.Owner {
		return result, nil
	}
	return result, journal.RetireConnectionWindow(ctx, parent, meta.Owner)
}
func (p *Project) Disconnect(ctx context.Context, id string, cache bool) (ProjectResult, error) {
	state, err := p.Status(id)
	if errors.Is(err, journal.ErrMemoryTornTail) {
		if repairErr := p.recoverTail(ctx, id); repairErr != nil {
			return state, repairErr
		}
		state, err = p.Status(id)
	}
	if err != nil {
		return state, err
	}
	if state.Closed {
		return p.retire(ctx, id, state)
	}
	meta, err := p.metadata(id)
	if err != nil {
		return state, err
	}
	ended, err := desktop.ProcessEnded(ctx, meta.Target.Window)
	if err != nil {
		return state, err
	}
	if ended != "" {
		return p.disconnectEnded(ctx, id, meta)
	}
	return p.drive(ctx, id, cache, func(d *Driver) error {
		if err := d.RequestClose(ctx); err != nil {
			return err
		}
		return d.continueOrRetire(ctx, d.Backend.(*Native), true)
	})
}
