//go:build windows && amd64

package duplexhost

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"math"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/follenfang/lycheedev/internal/buildinfo"
	"github.com/follenfang/lycheedev/internal/delivery"
	"github.com/follenfang/lycheedev/internal/live"
	"github.com/follenfang/lycheedev/internal/live/duplex"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/live/memory"
	"github.com/follenfang/lycheedev/internal/records"
	"github.com/follenfang/lycheedev/internal/vault"
)

type TargetRequest struct {
	Installation                     string
	PID                              uint32
	Character, Realm, Build, Product string
}
type Project struct{ Root string }
type targetRecord struct {
	Schema              string              `json:"schema"`
	Target              live.ClientWindow   `json:"target"`
	Claim               journal.WindowOwner `json:"claim"`
	ProcessClaimVersion uint32              `json:"processClaimVersion,omitempty"`
	Identity            duplex.Identity     `json:"identity"`
	ActorGUID           string              `json:"actorGUID"`
	ClaimRetired        bool                `json:"claimRetired"`
	RuntimeRetirement   *runtimeRetirement  `json:"runtimeRetirement,omitempty"`
}
type runtimeRetirement struct {
	FromRuntime string `json:"fromRuntime"`
	ToRuntime   string `json:"toRuntime"`
	MessageID   string `json:"messageId"`
	Quiescent   bool   `json:"quiescent"`
	RuntimeGone bool   `json:"runtimeGone"`
}
type requestRecord struct {
	Key          string                 `json:"key"`
	SourceSHA256 string                 `json:"sourceSHA256"`
	Budget       int                    `json:"budget"`
	Sequence     uint64                 `json:"sequence,string"`
	RequestID    string                 `json:"requestId,omitempty"`
	Result       *duplex.ResultManifest `json:"result,omitempty"`
	Complete     bool                   `json:"complete"`
}
type ProjectResult struct {
	Session             string                      `json:"session"`
	Operation           string                      `json:"operation,omitempty"`
	Identity            duplex.Identity             `json:"identity"`
	Bound               bool                        `json:"bound"`
	Closed              bool                        `json:"closed"`
	Stage               string                      `json:"stage"`
	Complete            bool                        `json:"complete"`
	ReportState         string                      `json:"reportState"`
	Cleanup             string                      `json:"cleanup"`
	Report              json.RawMessage             `json:"report,omitempty"`
	Journal             string                      `json:"journal"`
	Status              *duplex.Sendbox             `json:"status,omitempty"`
	NativeReload        *memory.ReloadObservation   `json:"nativeReload,omitempty"`
	Target              *live.ClientWindow          `json:"target,omitempty"`
	Diagnostics         map[string]Diagnostic       `json:"diagnostics,omitempty"`
	WriterProfile       *memory.DuplexWriterProfile `json:"writerProfile,omitempty"`
	WriterQualification *memory.DuplexQualification `json:"writerQualification,omitempty"`
}
type Diagnostic struct {
	State  string `json:"state"`
	Reason string `json:"reason,omitempty"`
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
		return nil, errors.New("live.duplex_project_invalid")
	}
	return &Project{Root: root}, nil
}
func ValidateRequest(key string) error {
	if len(key) < 1 || len(key) > 128 || strings.Trim(key, "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789._-") != "" {
		return errors.New("live.duplex_request_key_invalid")
	}
	return nil
}
func validateID(id string) error {
	if !strings.HasPrefix(id, "CON-") || len(id) != 36 || strings.Trim(id[4:], "0123456789abcdef") != "" {
		return errors.New("live.duplex_connection_id_required")
	}
	return nil
}
func validToken(token string) bool {
	return len(token) == 32 && strings.Trim(token, "0123456789abcdef") == ""
}
func (p *Project) path(id string) string {
	return filepath.Join(p.Root, ".lycheedev", "live", "duplex", id)
}
func (p *Project) workspaceID() string { return digestBytes([]byte(strings.ToLower(p.Root)))[:32] }
func boundedJSON(path string, value any, max int64) error {
	info, err := os.Lstat(path)
	if err != nil {
		return err
	}
	if !info.Mode().IsRegular() || info.Size() > max {
		return errors.New("live.duplex_record_invalid")
	}
	f, err := os.Open(path)
	if err != nil {
		return err
	}
	defer f.Close()
	b, err := io.ReadAll(io.LimitReader(f, max+1))
	if err != nil {
		return err
	}
	if int64(len(b)) > max {
		return errors.New("live.duplex_record_limit")
	}
	return json.Unmarshal(b, value)
}
func boundedBytes(path string, max int64) ([]byte, error) {
	info, e := os.Lstat(path)
	if e != nil {
		return nil, e
	}
	if !info.Mode().IsRegular() || info.Size() > max {
		return nil, errors.New("live.duplex_record_invalid")
	}
	f, e := os.Open(path)
	if e != nil {
		return nil, e
	}
	defer f.Close()
	p, e := io.ReadAll(io.LimitReader(f, max+1))
	if e == nil && int64(len(p)) > max {
		return nil, errors.New("live.duplex_record_limit")
	}
	return p, e
}
func writeJSON(ctx context.Context, path string, value any) error {
	b, err := json.Marshal(value)
	if err != nil {
		return err
	}
	if err = vault.ReplaceFile(ctx, path, b); err != nil {
		return errors.Join(duplex.ErrPersistence, err)
	}
	return nil
}
func (p *Project) metadata(id string) (targetRecord, error) {
	var meta targetRecord
	if err := validateID(id); err != nil {
		return meta, err
	}
	err := boundedJSON(filepath.Join(p.path(id), "target.json"), &meta, 64<<10)
	if err == nil && (meta.Schema != "lycheedev.duplex.target.v1" || meta.Claim.OperationID != id || meta.Claim.WorkspaceID != p.workspaceID()) {
		err = errors.New("live.duplex_target_invalid")
	}
	return meta, err
}
func addonParent(t live.ClientWindow) string {
	return filepath.Join(t.Client.Directory, "Interface", "AddOns")
}
func deployment(ctx context.Context, t live.ClientWindow) error {
	if err := live.ConfirmClientWindow(ctx, t); err != nil {
		return err
	}
	client, err := records.InspectClientInstallation(ctx, t.Client.Directory)
	if err != nil {
		return err
	}
	status, err := delivery.InspectInstallation(ctx, delivery.AddonDirectory(t.Client.Directory), "addon")
	if err != nil {
		return err
	}
	if client != t.Client || status.State != "managed" || status.Receipt == nil || status.Receipt.Version != buildinfo.Version {
		return errors.New("live.duplex_clean_current_addon_required")
	}
	return nil
}
func (p *Project) present(ctx context.Context, id string, st duplex.State, status *duplex.Sendbox) ProjectResult {
	r := ProjectResult{Session: id, Identity: st.Identity, Bound: st.Bound, Closed: st.Closed, Stage: "connecting", Complete: st.Selected || st.Bound, ReportState: "unavailable", Cleanup: "none", Journal: filepath.Join(p.path(id), "state.json"), Status: status}
	if st.Selected || st.Bound {
		r.Stage = "selected"
	}
	if st.LocalRetired {
		r.Stage = "retired"
		r.Complete = true
		r.Cleanup = "pending"
	}
	if st.Closed {
		r.Stage = "closed"
		r.Complete = true
	}
	if a := st.Active; a != nil {
		r.Operation = a.RequestID
		r.Stage = a.Phase
		r.Complete = a.ResultSaved
		r.Cleanup = "pending"
		if a.ResultSaved && a.Result != nil && validToken(a.RequestID) && a.Result.RequestID == a.RequestID {
			b, err := boundedBytes(filepath.Join(p.path(id), "result-"+a.RequestID+".bin"), duplex.MaxResultBytes)
			if err == nil && duplex.ValidateResult(*a.Result, b) == nil && json.Valid(b) {
				r.Report = b
				r.ReportState = "verified"
			}
		}
		if a.Released {
			r.Cleanup = "complete"
		}
		if st.Closed {
			r.Stage = "closed"
			r.Complete = a.Released
		}
	}
	if st.LocalRetired {
		r.Stage = "retired"
		r.Cleanup = "pending"
		r.Complete = st.Active == nil || st.Active.ResultSaved
		if st.Active != nil && !st.Active.ResultSaved {
			r.Stage = "execution_unknown"
		}
	}
	return r
}

func (p *Project) Connect(ctx context.Context, req TargetRequest, _ bool) (ProjectResult, error) {
	doctor, e := p.InspectTarget(ctx, req)
	if e != nil {
		return doctor, e
	}
	if doctor.WriterQualification == nil || !doctor.WriterQualification.DirectWrite {
		return doctor, errors.New("live.duplex_writer_profile_unverified")
	}
	if doctor.Diagnostics["runtimeFresh"].State != "advancing" || doctor.Diagnostics["actorReady"].State != "verified" || doctor.NativeReload == nil || doctor.NativeReload.CheckBusinessWriteGate() != nil {
		return doctor, duplex.ErrPending
	}
	target, err := live.ResolveClientWindow(ctx, req.Installation, req.PID)
	if err != nil {
		return ProjectResult{}, err
	}
	if req.Build != "" && req.Build != target.Client.FullBuild || req.Product != "" && req.Product != target.Client.Product {
		return ProjectResult{}, errors.New("live.duplex_target_mismatch")
	}
	resource := fmt.Sprintf("window/%d/%d/%d", target.Window.ProcessID, target.Window.ProcessStartedAt, target.Window.Handle)
	owner, busy, err := inspectConnectionOwner(ctx, target)
	if err != nil {
		return ProjectResult{}, err
	}
	if busy {
		if owner.WorkspaceID != p.workspaceID() {
			return ProjectResult{}, &journal.WindowOccupied{Owner: owner, Foreign: true}
		}
		meta, e := p.metadata(owner.OperationID)
		if e != nil {
			return ProjectResult{}, e
		}
		if meta.Target != target {
			return ProjectResult{}, duplex.ErrIdentity
		}
		resumed, resumeErr := p.Resume(ctx, owner.OperationID, false)
		latestMeta, metaErr := p.metadata(owner.OperationID)
		if metaErr == nil && latestMeta.ClaimRetired {
			return p.Connect(ctx, req, false)
		}
		if resumeErr != nil {
			return resumed, resumeErr
		}
		if req.Character != "" || req.Realm != "" {
			observed, e := p.Inspect(ctx, owner.OperationID)
			if e != nil {
				return observed, e
			}
			if observed.Status == nil || req.Character != "" && observed.Status.Character != req.Character || req.Realm != "" && observed.Status.Realm != req.Realm {
				return observed, duplex.ErrIdentity
			}
		}
		return resumed, nil
	}
	if err = deployment(ctx, target); err != nil {
		return ProjectResult{}, err
	}
	n, err := OpenNative(ctx, target)
	if err != nil {
		return ProjectResult{}, err
	}
	defer n.Close(ctx)
	s, err := n.Observe(ctx)
	if err != nil {
		return ProjectResult{}, err
	}
	if s.Build != target.Client.FullBuild || s.Product != target.Client.Product || s.Release != buildinfo.Version || s.ActorGUID == "" || req.Character != "" && s.Character != req.Character || req.Realm != "" && s.Realm != req.Realm {
		return ProjectResult{}, errors.New("live.duplex_actor_or_build_mismatch")
	}
	key, err := duplex.NewToken()
	if err != nil {
		return ProjectResult{}, err
	}
	session, err := duplex.NewToken()
	if err != nil {
		return ProjectResult{}, err
	}
	id := "CON-" + key
	identity, err := duplex.NextIdentity(s, key, session)
	if err != nil {
		return ProjectResult{Status: &s, Target: &target}, err
	}
	claim := journal.WindowOwner{Schema: "lycheedev.window-owner.v1", WorkspaceID: p.workspaceID(), Resource: resource, OperationID: id, IntentSHA256: digestBytes([]byte(resource + "/" + id))}
	meta := targetRecord{Schema: "lycheedev.duplex.target.v1", Target: target, Claim: claim, ProcessClaimVersion: 1, Identity: identity, ActorGUID: s.ActorGUID}
	err = beginConnectionClaim(ctx, target, claim, func() error {
		if e := duplex.NewFileStore(p.path(id)).Update(ctx, func(st *duplex.State) error { st.Identity = identity; return nil }); e != nil {
			return e
		}
		return writeJSON(ctx, filepath.Join(p.path(id), "target.json"), meta)
	})
	if err != nil {
		return ProjectResult{}, err
	}
	n.TraceDir = filepath.Join(p.path(id), "writes")
	n.Guard = p.guard(meta, id)
	n.BatchGuard = p.lightGuard(meta)
	n.ExpectedActorGUID = meta.ActorGUID
	lock, err := lockConnectionDriver(ctx, target, claim)
	if err != nil {
		return ProjectResult{Session: id}, err
	}
	defer lock.Close()
	c := duplex.NewCoordinator(n, duplex.NewFileStore(p.path(id)))
	st, err := c.Connect(ctx, identity)
	st, err = reloadCurrent(ctx, c.Store, st, err)
	return p.present(ctx, id, st, nil), err
}

func (p *Project) guard(meta targetRecord, id string) func(context.Context, duplex.Kind) error {
	return func(ctx context.Context, kind duplex.Kind) error {
		if err := deployment(ctx, meta.Target); err != nil {
			return err
		}
		if err := p.lightGuard(meta)(ctx, kind); err != nil {
			return err
		}
		if kind == duplex.Frame || kind == duplex.Commit {
			st, e := duplex.NewFileStore(p.path(id)).Load(ctx)
			if e != nil {
				return e
			}
			return businessControlGuard(st)
		}
		return nil
	}
}

func (p *Project) lightGuard(meta targetRecord) func(context.Context, duplex.Kind) error {
	return func(ctx context.Context, _ duplex.Kind) error {
		if e := live.ConfirmClientWindow(ctx, meta.Target); e != nil {
			return e
		}
		return verifyConnectionClaim(ctx, meta.Target, meta.Claim)

	}
}
func businessControlGuard(st duplex.State) error {
	if st.Closing {
		return errors.New("live.duplex_closing")
	}
	if st.ReloadPrepared != nil {
		return errors.New("live.duplex_reload_pending")
	}
	if st.Closed {
		return errors.New("live.duplex_closing")
	}
	if in, ok := st.Intents["stop"]; ok {
		if in.Message.Header.Kind == duplex.Close {
			return errors.New("live.duplex_closing")
		}
		if !in.Accepted {
			return errors.New("live.duplex_stop_pending")
		}
	}
	return nil
}
func reloadCurrent(ctx context.Context, store duplex.Store, fallback duplex.State, actionErr error) (duplex.State, error) {
	durable, cancel := context.WithTimeout(context.WithoutCancel(ctx), 5*time.Second)
	defer cancel()
	latest, e := store.Load(durable)
	if e != nil {
		return fallback, errors.Join(actionErr, e)
	}
	return latest, actionErr
}
func (p *Project) drive(ctx context.Context, id string, control bool, action func(*duplex.Coordinator) (duplex.State, error)) (result ProjectResult, returned error) {
	meta, err := p.metadata(id)
	if err != nil {
		return result, err
	}
	if meta.ProcessClaimVersion != 1 {
		return result, legacyClaimError()
	}
	store := duplex.NewFileStore(p.path(id))
	before, err := store.Load(ctx)
	if err != nil {
		return result, err
	}
	if proof := meta.RuntimeRetirement; proof != nil {
		in := before.ReloadPrepared
		ok := in != nil
		if !ok || (!in.Accepted && !proof.RuntimeGone) || in.Message.Header.MessageID != proof.MessageID || proof.FromRuntime != before.Identity.Runtime || !validToken(proof.ToRuntime) || proof.ToRuntime == proof.FromRuntime || !proof.Quiescent || before.Active != nil && !before.Active.Released {
			return result, duplex.ErrUnknown
		}
		if !before.Closed {
			if e := store.Update(ctx, func(st *duplex.State) error { st.Closed = true; return nil }); e != nil {
				return result, e
			}
			before.Closed = true
		}
	}
	if meta.ClaimRetired {
		retired := p.present(ctx, id, before, nil)
		if before.LocalRetirement != nil {
			retired.Cleanup = "local_retired"
		} else {
			retired.Cleanup = "complete"
		}
		return retired, nil
	}
	if before.LocalRetired {
		return p.retireLocalSelection(ctx, id, meta, before)
	}
	doctor, doctorErr := p.Inspect(ctx, id)
	if doctor.Status != nil && doctor.Diagnostics["processIdentity"].State == "verified" && doctor.Diagnostics["runtimeFresh"].State == "advancing" && observedLifecycleChanged(meta, before, *doctor.Status) {
		if before.ReloadPrepared == nil || before.Active != nil && !before.Active.Released {
			return p.retireChangedRuntime(ctx, id, meta, store, before, doctor)
		}
	}
	if doctorErr != nil {
		retiringReload := before.ReloadPrepared != nil && (before.Active == nil || before.Active.Released) && doctor.Status != nil && doctor.Status.Runtime != before.Identity.Runtime && errors.Is(doctorErr, duplex.ErrIdentity)
		if !retiringReload {
			return doctor, doctorErr
		}
	}
	if doctor.Status == nil || doctor.Diagnostics["runtimeFresh"].State != "advancing" || doctor.Status.ActorBinding != before.Identity.ActorBinding || (!control && doctor.Status.ActorGUID != meta.ActorGUID || control && doctor.Status.ActorGUID != "" && doctor.Status.ActorGUID != meta.ActorGUID) {
		return doctor, duplex.ErrIdentity
	}
	if doctor.NativeReload == nil || doctor.NativeReload.CheckWriteGate() != nil {
		return doctor, duplex.ErrPending
	}
	var driver *connectionDriver
	if !control {
		driver, err = lockConnectionDriver(ctx, meta.Target, meta.Claim)
		if err != nil {
			return p.present(ctx, id, before, nil), err
		}
		defer func() {
			if driver != nil {
				returned = errors.Join(returned, driver.Close())
			}
		}()
	}
	n, err := OpenNative(ctx, meta.Target)
	if err != nil {
		return p.present(ctx, id, before, nil), err
	}
	n.TraceDir = filepath.Join(p.path(id), "writes")
	n.Guard = p.guard(meta, id)
	n.BatchGuard = p.lightGuard(meta)
	n.ExpectedActorGUID = meta.ActorGUID
	defer func() { returned = errors.Join(returned, n.Close(context.WithoutCancel(ctx))) }()
	c := duplex.NewCoordinator(n, store)
	if doctor.Status.Repair != nil && doctor.Status.Arena != before.Identity.Arena {
		if _, e := n.repair(ctx, c); e != nil {
			return p.present(ctx, id, before, doctor.Status), e
		}
		if _, e := c.Resume(ctx); e != nil {
			return p.present(ctx, id, before, doctor.Status), e
		}
	}
	var st duplex.State
	if before.Closed && (before.Active == nil || before.Active.Released) {
		st = before
	} else {
		st, err = action(c)
	}
	if err != nil {
		st, err = reloadCurrent(ctx, store, st, err)
	}
	result = p.present(ctx, id, st, nil)
	if st.LocalRetired {
		if driver != nil {
			if e := driver.Close(); e != nil {
				return result, errors.Join(err, e)
			}
			driver = nil
		}
		if e := n.Close(ctx); e != nil {
			return result, errors.Join(err, e)
		}
		retired, e := p.retireLocalSelection(ctx, id, meta, st)
		return retired, errors.Join(err, e)
	}
	if st.Closed {
		latestMeta, e := p.metadata(id)
		if e != nil {
			return result, errors.Join(err, e)
		}
		meta = latestMeta
		// The short native write handles are already gone. A still-running
		// business driver prevents retirement instead of deleting its claim.
		if driver != nil {
			if e := driver.Close(); e != nil {
				return result, errors.Join(err, e)
			}
			driver = nil
		}
		if e := n.Close(ctx); e != nil {
			return result, errors.Join(err, e)
		}
		// Closing new work is not release. Failed result reads or unfinished
		// resources keep this exact durable claim for later cleanup.
		if st.Active != nil && !st.Active.Released {
			result.Cleanup = "pending"
			return result, errors.Join(err, duplex.ErrPending)
		}
		fresh, e := OpenNative(ctx, meta.Target)
		if e != nil {
			return result, errors.Join(err, e)
		}
		defer fresh.Close(ctx)
		s, e := fresh.Observe(ctx)
		if e != nil {
			return result, errors.Join(err, e)
		}
		result.Status = &s
		retirementValid := s.Identity == st.Identity && (s.Phase == "closed" || s.Phase == "ready_unbound" && s.ClosedAdmission) && s.ResourcesReleased
		if proof := meta.RuntimeRetirement; proof != nil {
			retirementValid = s.Runtime == proof.ToRuntime && s.Runtime != proof.FromRuntime && proof.Quiescent
		}
		if !retirementValid {
			return result, errors.Join(err, duplex.ErrPending)
		}
		release, e := fresh.WritersDrained(ctx)
		if e != nil {
			return result, errors.Join(err, e)
		}
		defer release()
		if e = retireConnectionClaim(ctx, meta.Target, meta.Claim); e != nil {
			return result, errors.Join(err, e)
		}
		meta.ClaimRetired = true
		if e = writeJSON(ctx, filepath.Join(p.path(id), "target.json"), meta); e != nil {
			return result, errors.Join(err, e)
		}
		result.Cleanup = "complete"
	}
	return result, err
}
func (p *Project) Status(id string) (ProjectResult, error) {
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	return p.Inspect(ctx, id)
}
func (p *Project) Resume(ctx context.Context, id string, _ bool) (ProjectResult, error) {
	return p.drive(ctx, id, false, func(c *duplex.Coordinator) (duplex.State, error) {
		st, e := c.Store.Load(ctx)
		if e != nil {
			return st, e
		}
		if !st.Selected && !st.Bound && st.Active == nil {
			return c.Connect(ctx, st.Identity)
		}
		n := c.Backend.(*Native)
		s, e := n.Observe(ctx)
		if e != nil {
			return st, e
		}
		if s.Runtime != st.Identity.Runtime {
			if st.ReloadPrepared != nil && (st.Active == nil || st.Active.Released) {
				return p.saveRuntimeRetirement(ctx, id, c.Store, st, s)
			}
		}
		if st.ReloadPrepared != nil {
			return p.resumeReload(ctx, id, c)
		}
		if in, ok := st.Intents["stop"]; ok && (in.Message.Header.Kind == duplex.Reload || in.Message.Header.Kind == duplex.Lease) {
			return p.resumeReload(ctx, id, c)
		}
		if s.Runtime == st.Identity.Runtime && s.Arena != st.Identity.Arena && s.Repair != nil {
			if _, e = n.repair(ctx, c); e != nil {
				return st, e
			}
		}
		return c.Resume(ctx)
	})
}
func (p *Project) Cancel(ctx context.Context, id string) (ProjectResult, error) {
	return p.drive(ctx, id, true, func(c *duplex.Coordinator) (duplex.State, error) { return c.Cancel(ctx, 15*time.Second) })
}
func (p *Project) Disconnect(ctx context.Context, id string, _ bool) (ProjectResult, error) {
	// This preflight is entirely local. A selected session with no publication
	// has no addon ownership to close, even if the target stopped or exited.
	meta, e := p.metadata(id)
	if e != nil {
		return ProjectResult{}, e
	}
	store := duplex.NewFileStore(p.path(id))
	before, e := store.Load(ctx)
	if e != nil {
		return ProjectResult{}, e
	}
	if selectedWithoutPublication(before) {
		if meta.ClaimRetired {
			result := p.present(ctx, id, before, nil)
			result.Cleanup = "complete"
			return result, nil
		}
		e = store.Update(ctx, func(current *duplex.State) error {
			if current.Identity != before.Identity || current.Identity != meta.Identity {
				return duplex.ErrIdentity
			}
			if !selectedWithoutPublication(*current) {
				return duplex.ErrPending
			}
			current.LocalRetired = true
			current.Closing = true
			return nil
		})
		if e != nil {
			latest, loadErr := store.Load(ctx)
			return p.present(ctx, id, latest, nil), errors.Join(e, loadErr)
		}
		retired, e := store.Load(ctx)
		if e != nil {
			return ProjectResult{}, e
		}
		return p.retireLocalSelection(ctx, id, meta, retired)
	}
	return p.drive(ctx, id, true, func(c *duplex.Coordinator) (duplex.State, error) { return c.Disconnect(ctx, 30*time.Second) })
}
func selectedWithoutPublication(st duplex.State) bool {
	return st.Selected && !st.Bound && st.Active == nil && len(st.Intents) == 0 && st.PublicationSequence == 0 && st.RequestSequence == 0 && st.ReloadPrepared == nil && st.LocalRetirement == nil && !st.Closed
}
func (p *Project) Reload(ctx context.Context, id, key string, _ bool) (ProjectResult, error) {
	if err := ValidateRequest(key); err != nil {
		return ProjectResult{}, err
	}
	ctx, cancel := context.WithTimeout(ctx, 30*time.Second)
	defer cancel()
	return p.drive(ctx, id, true, func(c *duplex.Coordinator) (duplex.State, error) {
		st, e := c.Store.Load(ctx)
		if e != nil {
			return st, e
		}
		if st.Active != nil && !st.Active.ResultSaved {
			return st, duplex.ErrBusy
		}
		meta, e := p.metadata(id)
		if e != nil {
			return st, e
		}
		driver, e := lockConnectionDriver(ctx, meta.Target, meta.Claim)
		if e != nil {
			return st, e
		}
		defer driver.Close()
		n := c.Backend.(*Native)
		before, e := n.Observe(ctx)
		if e != nil {
			return st, e
		}
		if !inspectionIdentity(st, before) || !before.ResourcesReleased {
			return st, duplex.ErrPending
		}
		return p.resumeReload(ctx, id, c)

	})
}

func (p *Project) saveRuntimeRetirement(ctx context.Context, id string, store duplex.Store, st duplex.State, s duplex.Sendbox) (duplex.State, error) {
	in := st.ReloadPrepared
	if in == nil || !validToken(s.Runtime) || s.Runtime == st.Identity.Runtime || st.Active != nil && !st.Active.Released {
		return st, duplex.ErrUnknown
	}
	meta, e := p.metadata(id)
	if e != nil {
		return st, e
	}
	meta.RuntimeRetirement = &runtimeRetirement{FromRuntime: st.Identity.Runtime, ToRuntime: s.Runtime, MessageID: in.Message.Header.MessageID, Quiescent: true, RuntimeGone: true}
	if e = writeJSON(ctx, filepath.Join(p.path(id), "target.json"), meta); e != nil {
		return st, e
	}
	if e = store.Update(ctx, func(current *duplex.State) error {
		if current.Identity != st.Identity || current.Active != nil && !current.Active.Released {
			return duplex.ErrUnknown
		}
		current.Closed = true
		return nil
	}); e != nil {
		return st, e
	}
	return store.Load(ctx)
}
func (p *Project) Execute(ctx context.Context, id, key, code string, budget int, _ bool) (ProjectResult, error) {
	if err := ValidateRequest(key); err != nil {
		return ProjectResult{}, err
	}
	if len(code) < 1 || len(code) > duplex.MaxSourceBytes || budget < 1 || budget > 120 {
		return ProjectResult{}, errors.New("live.duplex_request_invalid")
	}
	return p.drive(ctx, id, false, func(c *duplex.Coordinator) (duplex.State, error) {
		path := filepath.Join(p.path(id), "requests", digestBytes([]byte(key))+".json")
		var record requestRecord
		st, err := c.Store.Load(ctx)
		if err != nil {
			return st, err
		}
		err = boundedJSON(path, &record, 16<<10)
		if err == nil {
			if record.Key != key || record.SourceSHA256 != digestBytes([]byte(code)) || record.Budget != budget || record.Sequence == 0 || record.RequestID != "" && !validToken(record.RequestID) {
				return st, errors.New("live.duplex_request_conflict")
			}
			if record.Complete && record.Result != nil {
				if record.Result.RequestID != record.RequestID {
					return st, errors.New("live.duplex_request_conflict")
				}
				b, e := boundedBytes(filepath.Join(p.path(id), "result-"+record.RequestID+".bin"), duplex.MaxResultBytes)
				if e != nil {
					return st, e
				}
				if e = duplex.ValidateResult(*record.Result, b); e != nil {
					return st, e
				}
				return duplex.State{Identity: st.Identity, Bound: st.Bound, Active: &duplex.ActiveRequest{RequestID: record.RequestID, Result: record.Result, ResultSaved: true, Released: historicalResultReleased(st, record), Phase: "result_pending"}}, nil
			}
			if st.Active != nil && st.Active.Sequence == record.Sequence {
				if e := matchRequestRecord(record, st, []byte(code)); e != nil {
					return st, e
				}
				st, err = c.Resume(ctx)
			} else if st.RequestSequence < math.MaxUint64 && st.RequestSequence+1 == record.Sequence {
				st, err = c.Execute(ctx, []byte(code), uint32(budget*1000))
			} else {
				return st, duplex.ErrUnknown
			}
		} else if os.IsNotExist(err) {
			if st.Active != nil && !st.Active.ResultSaved {
				return st, duplex.ErrBusy
			}
			if st.RequestSequence == math.MaxUint64 {
				return st, errors.New("live.duplex_request_sequence_exhausted")
			}
			record = requestRecord{Key: key, SourceSHA256: digestBytes([]byte(code)), Budget: budget, Sequence: st.RequestSequence + 1}
			if err = writeJSON(ctx, path, record); err != nil {
				return st, err
			}
			st, err = c.Execute(ctx, []byte(code), uint32(budget*1000))
		} else {
			return st, err
		}
		if err != nil {
			st, err = reloadCurrent(ctx, c.Store, st, err)
		}
		if a := st.Active; a != nil && a.Sequence == record.Sequence {
			if e := matchRequestRecord(record, st, []byte(code)); e != nil {
				return st, errors.Join(err, e)
			}
			record.RequestID = a.RequestID
			record.Result = a.Result
			record.Complete = a.ResultSaved
			durable, cancel := context.WithTimeout(context.WithoutCancel(ctx), 5*time.Second)
			defer cancel()
			err = errors.Join(err, writeJSON(durable, path, record))
		}
		return st, err
	})
}

func matchRequestRecord(record requestRecord, st duplex.State, source []byte) error {
	a := st.Active
	if record.Sequence == 0 || a == nil || a.Sequence != record.Sequence || a.Budget != uint32(record.Budget*1000) || record.SourceSHA256 != digestBytes(source) || record.RequestID != "" && record.RequestID != a.RequestID {
		return errors.New("live.duplex_request_conflict")
	}
	h := duplex.Header{RequestID: a.RequestID, ActorBinding: st.Identity.ActorBinding, CreatedUTCMillis: a.Created, BudgetMillis: a.Budget, TotalBytes: uint32(len(source))}
	d, e := duplex.RequestDigest(h, source)
	if e != nil || d != a.Digest {
		return errors.New("live.duplex_request_conflict")
	}
	return nil
}

func (p *Project) resumeReload(ctx context.Context, id string, c *duplex.Coordinator) (duplex.State, error) {
	n := c.Backend.(*Native)
	st, e := c.Reload(ctx, 30*time.Second)
	if e != nil {
		st, e = reloadCurrent(ctx, c.Store, st, e)
		if s, observeErr := n.Observe(ctx); observeErr == nil && s.Runtime != st.Identity.Runtime && (st.Active == nil || st.Active.Released) {
			retired, saveErr := p.saveRuntimeRetirement(ctx, id, c.Store, st, s)
			if saveErr == nil {
				return retired, nil
			}
			return retired, errors.Join(e, saveErr)
		}
		return st, e
	}
	in := st.ReloadPrepared
	if in == nil || !in.Accepted {
		return st, duplex.ErrUnknown
	}
	st, e = c.CommitReload(ctx, 15*time.Second)
	if e != nil {
		st, e = reloadCurrent(ctx, c.Store, st, e)
		if s, observeErr := n.Observe(ctx); observeErr == nil && s.Runtime != st.Identity.Runtime && (st.Active == nil || st.Active.Released) {
			retired, saveErr := p.saveRuntimeRetirement(ctx, id, c.Store, st, s)
			if saveErr == nil {
				return retired, nil
			}
			return retired, errors.Join(e, saveErr)
		}
		return st, e
	}
	for {
		s, observeErr := n.Observe(ctx)
		if observeErr == nil && s.Runtime != st.Identity.Runtime {
			return p.saveRuntimeRetirement(ctx, id, c.Store, st, s)
		}
		timer := time.NewTimer(25 * time.Millisecond)
		select {
		case <-ctx.Done():
			timer.Stop()
			return st, duplex.ErrPending
		case <-timer.C:
		}
	}
}

func historicalResultReleased(st duplex.State, record requestRecord) bool {
	a := st.Active
	if a == nil {
		return false
	}
	if a.RequestID == record.RequestID {
		return a.Released
	}
	// Staging the next request is not proof its joint ACK was accepted.
	if a.Sequence <= record.Sequence {
		return false
	}
	if a.Sequence-record.Sequence > 1 {
		return true
	}
	return a.PreviousResultAck != nil && a.PreviousResultAck.RequestID == record.RequestID && a.NextFrame == 1
}

// A selected session with no publication has no in-game ownership to close.
// Retire only its exact local claim after all physical writers are drained.
func (p *Project) retireLocalSelection(ctx context.Context, id string, meta targetRecord, st duplex.State) (ProjectResult, error) {
	if meta.ProcessClaimVersion != 1 {
		return p.present(ctx, id, st, nil), legacyClaimError()
	}
	result := p.present(ctx, id, st, nil)
	if !st.LocalRetired || (st.LocalRetirement == nil && (st.Bound || st.Active != nil || len(st.Intents) != 0 || st.PublicationSequence != 0)) {
		return result, duplex.ErrUnknown
	}
	native := &Native{Target: meta.Target}
	release, e := native.WritersDrained(ctx)
	if e != nil {
		return result, e
	}
	defer release()
	if e = retireConnectionClaim(ctx, meta.Target, meta.Claim); e != nil {
		return result, e
	}
	meta.ClaimRetired = true
	if e = writeJSON(ctx, filepath.Join(p.path(id), "target.json"), meta); e != nil {
		return result, e
	}
	if st.LocalRetirement != nil {
		result.Cleanup = "local_retired"
	} else {
		result.Cleanup = "complete"
	}
	return result, nil
}

func observedLifecycleChanged(meta targetRecord, st duplex.State, s duplex.Sendbox) bool {
	return s.Runtime != st.Identity.Runtime || s.ActorBinding != st.Identity.ActorBinding || s.ActorReady && s.ActorGUID != "" && s.ActorGUID != meta.ActorGUID
}
func (p *Project) retireChangedRuntime(ctx context.Context, id string, meta targetRecord, store duplex.Store, st duplex.State, doctor ProjectResult) (ProjectResult, error) {
	if meta.ProcessClaimVersion != 1 {
		return doctor, legacyClaimError()
	}
	s := doctor.Status
	if s == nil || doctor.Target == nil || *doctor.Target != meta.Target || doctor.Diagnostics["processIdentity"].State != "verified" || doctor.Diagnostics["runtimeFresh"].State != "advancing" || !observedLifecycleChanged(meta, st, *s) {
		return doctor, duplex.ErrUnknown
	}
	proof := duplex.LocalRetirementProof{ProcessID: meta.Target.Window.ProcessID, ProcessCreated: meta.Target.Window.ProcessStartedAt, Executable: meta.Target.Window.Executable, PreviousRuntime: st.Identity.Runtime, ObservedRuntime: s.Runtime, PreviousActorBinding: st.Identity.ActorBinding, ObservedActorBinding: s.ActorBinding, PreviousActorGUID: meta.ActorGUID, ObservedActorGUID: s.ActorGUID}
	if e := store.Update(ctx, func(current *duplex.State) error {
		if current.Identity != st.Identity {
			return duplex.ErrIdentity
		}
		current.LocalRetired = true
		current.Closing = true
		current.Closed = false
		current.LocalRetirement = &proof
		return nil
	}); e != nil {
		return doctor, e
	}
	latest, e := store.Load(ctx)
	if e != nil {
		return doctor, e
	}
	return p.retireLocalSelection(ctx, id, meta, latest)
}
