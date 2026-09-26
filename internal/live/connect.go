package live

import (
	"context"
	"errors"
	"fmt"
	"image"
	"strings"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/buildinfo"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/selection"
)

var (
	ErrCandidateAmbiguous = errors.New("live.candidate_ambiguous")
	ErrCandidateMissing   = errors.New("live.candidate_missing")
	ErrInputNotReady      = errors.New("live.input_not_ready")
	ErrActorChanged       = errors.New("live.actor_changed")
	ErrIdentityUnreadable = errors.New("live.identity_unreadable")
	ErrSessionConstraint  = errors.New("live.session_constraint_mismatch")
)

// CandidateSelectionError carries the full candidate list as data so the user
// can choose and retry with tighter constraints. It never guesses by ordinal.
type CandidateSelectionError struct {
	Candidates []Candidate
	ambiguity  bool
}

func (e *CandidateSelectionError) Error() string {
	if e.ambiguity {
		return ErrCandidateAmbiguous.Error()
	}
	return ErrCandidateMissing.Error()
}

func (e *CandidateSelectionError) Unwrap() error {
	if e.ambiguity {
		return ErrCandidateAmbiguous
	}
	return ErrCandidateMissing
}

// InputNotReadyError reports why automatic input was refused. It is a precise
// observed state, not a guess, and the caller may retry later.
type InputNotReadyError struct {
	Reason string
}

func (e *InputNotReadyError) Error() string { return ErrInputNotReady.Error() + ": " + e.Reason }
func (e *InputNotReadyError) Unwrap() error { return ErrInputNotReady }

// ConnectRequest carries user constraints and the optional previous session to
// revive. Probe nonces, protocol stages and capture mechanics stay inside this
// module; a resumed request can never override the recorded identity.
type ConnectRequest struct {
	Snapshot     string
	Character    string
	Realm        string
	PID          uint32
	Installation string
	Session      string
	CaptureArea  image.Rectangle
}

func (r ConnectRequest) Validate() error {
	if strings.TrimSpace(r.Session) == "" && r.Snapshot == "" {
		return errors.New("live.connect_requires_snapshot")
	}
	if len(r.Session) > 128 {
		return errors.New("live.invalid_session_id")
	}
	for _, label := range []string{r.Character, r.Realm} {
		if !bindingLabel(label) {
			return errors.New("live.invalid_binding_identity")
		}
	}
	return validateCaptureArea(r.CaptureArea)
}

func validateCaptureArea(region image.Rectangle) error {
	if region == (image.Rectangle{}) {
		return nil
	}
	if region.Empty() || region.Min.X < 0 || region.Min.Y < 0 || region.Dx() > 4096 || region.Dy() > 4096 || region.Max.X > 16384 || region.Max.Y > 16384 {
		return errors.New("live.invalid_binding_region")
	}
	return nil
}

// ConnectWindow performs the whole first-contact task: discover candidates,
// identify them, resolve the caller's constraints, re-verify the chosen process
// and actor, then connect and persist the session through the existing binding
// store. A unique match connects automatically; genuine ambiguity is returned
// as data. Identity marking is the only game action before the choice.
func ConnectWindow(ctx context.Context, root string, request ConnectRequest) (Connection, error) {
	return connectWindow(ctx, root, request, nativeIO())
}

func connectWindow(ctx context.Context, root string, request ConnectRequest, io *liveIO) (Connection, error) {
	if err := request.Validate(); err != nil {
		return Connection{}, err
	}
	ctx, cancel := context.WithTimeout(ctx, 2*time.Minute)
	defer cancel()
	if request.Session != "" {
		return reviveConnection(ctx, root, request, io)
	}
	return firstConnection(ctx, root, request, io)
}

func firstConnection(ctx context.Context, root string, request ConnectRequest, io *liveIO) (Connection, error) {
	pin, err := selection.InspectSelection(ctx, root, request.Snapshot)
	if err != nil {
		return Connection{}, err
	}
	report, err := discoverCandidates(ctx, root, DiscoveryRequest{PID: request.PID, Installation: request.Installation}, io)
	if err != nil {
		return Connection{}, err
	}
	chosen, err := selectCandidate(report.Candidates, request)
	if err != nil {
		return Connection{}, err
	}
	target := ClientWindow{Client: chosen.Client, Window: chosen.Window}
	if err := sessionSnapshotMatches(pin, target); err != nil {
		return Connection{}, err
	}
	return connectCandidate(ctx, root, request.Snapshot, request.CaptureArea, chosen, io)
}

func selectCandidate(candidates []Candidate, request ConnectRequest) (Candidate, error) {
	matching := make([]Candidate, 0, 1)
	for _, candidate := range candidates {
		if candidate.State != CandidateIdentified {
			continue
		}
		if request.PID != 0 && candidate.Window.ProcessID != request.PID {
			continue
		}
		if request.Installation != "" && !sameInstallationPath(candidate.Client.Directory, request.Installation) {
			continue
		}
		if request.Character != "" && candidate.Character != request.Character {
			continue
		}
		if request.Realm != "" && candidate.Realm != request.Realm {
			continue
		}
		matching = append(matching, candidate)
	}
	switch len(matching) {
	case 1:
		return matching[0], nil
	case 0:
		return Candidate{}, &CandidateSelectionError{Candidates: candidates}
	default:
		return Candidate{}, &CandidateSelectionError{Candidates: candidates, ambiguity: true}
	}
}

// connectCandidate re-verifies the chosen process and actor with a fresh
// identity receipt before any effect, gates on observed input readiness, then
// performs the single opt-in and archives the ready handshake through the
// existing session machinery.
func connectCandidate(ctx context.Context, root, snapshot string, region image.Rectangle, chosen Candidate, io *liveIO) (Connection, error) {
	target := ClientWindow{Client: chosen.Client, Window: chosen.Window}
	// Re-verify before any effect: never connect to a closed or reused handle.
	if err := io.confirm(ctx, target); err != nil {
		return Connection{}, err
	}
	workspaceID := workspaceIdentity(ctx, root)
	owner, occupied, foreign, _ := windowOwnership(ctx, workspaceID, target, io)
	if occupied {
		return Connection{}, &journal.WindowOccupied{Owner: owner, Foreign: foreign}
	}
	observation, err := probeIdentity(ctx, target, region, true, io)
	if err != nil {
		return Connection{}, identityUnreadable(err)
	}
	identity := observation.Signal
	if !identity.InputReady {
		return Connection{}, &InputNotReadyError{Reason: notReadyReason(identity)}
	}
	if identity.ActorState != "ok" || identity.GUID != chosen.GUID || identity.Character != chosen.Character || identity.Realm != chosen.Realm {
		return Connection{}, fmt.Errorf("%w: window actor changed", ErrActorChanged)
	}
	if _, err := io.send(ctx, target.Window, "/dev connect"); err != nil {
		return Connection{}, err
	}
	frames, err := io.capture(ctx, target.Window, region)
	if err != nil {
		return Connection{}, err
	}
	defer frames.Close()
	expected := bridge.SignalExpectation{Kind: "ready", Release: buildinfo.Version, Character: identity.Character, Realm: identity.Realm, Product: target.Client.Product, Build: target.Client.FullBuild, RequireInputReady: true}
	session, err := observeWindowSessionAt(ctx, target, region, expected, sessionSignalIdentity(identity), frames, io.confirm)
	if err != nil {
		return Connection{}, err
	}
	defer session.Close()
	session.region = region
	if session.Ready().GUID != identity.GUID {
		return Connection{}, fmt.Errorf("%w: ready handshake actor changed", ErrActorChanged)
	}
	record, err := SaveWindowSession(ctx, root, snapshot, session)
	if err != nil {
		return Connection{}, err
	}
	return RecordedSession{Record: record, Target: session.Target(), Ready: session.Ready()}.Connection(), nil
}

// reviveConnection validates a recorded session against the live window and
// actor. Still valid returns the recorded connection unchanged; stale runs the
// same connect flow against the recorded constraints and keeps the old record
// as history. Interrupted operations are never re-run here.
func reviveConnection(ctx context.Context, root string, request ConnectRequest, io *liveIO) (Connection, error) {
	bound, err := ReadWindowSession(ctx, root, request.Session)
	if err != nil {
		return Connection{}, err
	}
	if err := reviveConstraints(request, bound); err != nil {
		return Connection{}, err
	}
	if connection, valid := revalidateConnection(ctx, root, bound, io); valid {
		return connection, nil
	}
	reconnect := ConnectRequest{
		Snapshot:     bound.Record.Snapshot,
		Character:    bound.Ready.Character,
		Realm:        bound.Ready.Realm,
		Installation: bound.Target.Client.Directory,
		CaptureArea:  bound.Record.Region,
	}
	if err := reconnect.Validate(); err != nil {
		return Connection{}, err
	}
	return firstConnection(ctx, root, reconnect, io)
}

func revalidateConnection(ctx context.Context, root string, bound RecordedSession, io *liveIO) (Connection, bool) {
	if err := io.confirm(ctx, bound.Target); err != nil {
		return Connection{}, false
	}
	workspaceID := workspaceIdentity(ctx, root)
	if _, occupied, _, err := windowOwnership(ctx, workspaceID, bound.Target, io); occupied || err != nil {
		return Connection{}, false
	}
	observation, err := probeIdentity(ctx, bound.Target, bound.Record.Region, false, io)
	if err != nil {
		return Connection{}, false
	}
	identity := observation.Signal
	if identity.ActorState != "ok" || identity.GUID != bound.Ready.GUID || identity.Character != bound.Ready.Character || identity.Realm != bound.Ready.Realm {
		return Connection{}, false
	}
	return bound.Connection(), true
}

func reviveConstraints(request ConnectRequest, bound RecordedSession) error {
	mismatch := false
	mismatch = mismatch || request.Snapshot != "" && request.Snapshot != bound.Record.Snapshot
	mismatch = mismatch || request.Character != "" && request.Character != bound.Ready.Character
	mismatch = mismatch || request.Realm != "" && request.Realm != bound.Ready.Realm
	mismatch = mismatch || request.Installation != "" && !sameInstallationPath(request.Installation, bound.Target.Client.Directory)
	mismatch = mismatch || request.PID != 0 && request.PID != bound.Target.Window.ProcessID
	mismatch = mismatch || request.CaptureArea != (image.Rectangle{}) && request.CaptureArea != bound.Record.Region
	if mismatch {
		return fmt.Errorf("%w: a revived session keeps its recorded identity", ErrSessionConstraint)
	}
	return nil
}
