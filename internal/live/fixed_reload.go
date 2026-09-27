package live

import (
	"bytes"
	"context"
	"crypto/rand"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"image"
	"path/filepath"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/buildinfo"
	"github.com/follenfang/lycheedev/internal/delivery"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/vault"
)

type FixedReloadRequest struct {
	Installation string
	PID          uint32
	Request      string
	Session      string
	WakeBinding  string
}

func (r FixedReloadRequest) Validate() error {
	if r.Installation == "" || r.PID == 0 || r.Request == "" || len(r.Request) > 128 || len(r.Session) > 128 {
		return errors.New("live.fixed_reload_selection_required")
	}
	if _, err := requestedReceiverBindings(r.WakeBinding); err != nil {
		return err
	}
	return nil
}

type fixedReloadIntent struct {
	Schema         string                   `json:"schema"`
	Target         ClientWindow             `json:"target"`
	Release        string                   `json:"release"`
	Commit         string                   `json:"commit"`
	IdentityNonce  string                   `json:"identityNonce"`
	OldEpoch       uint64                   `json:"oldEpoch,omitempty"`
	OldReportScope string                   `json:"oldReportScope,omitempty"`
	OldGUID        string                   `json:"oldGuid,omitempty"`
	OldCharacter   string                   `json:"oldCharacter,omitempty"`
	OldRealm       string                   `json:"oldRealm,omitempty"`
	Bindings       desktop.ReceiverBindings `json:"bindings,omitempty"`
	KnownBindings  bool                     `json:"knownBindings,omitempty"`
}

type fixedReloadObservation struct {
	Schema           string                `json:"schema"`
	DismissRequested bool                  `json:"dismissRequested,omitempty"`
	CloseQueued      bool                  `json:"closeQueued,omitempty"`
	ReleaseAt        time.Time             `json:"releaseAt,omitempty"`
	ReceiverReleased bool                  `json:"receiverReleased,omitempty"`
	NextStep         int                   `json:"nextStep,omitempty"`
	InputComplete    bool                  `json:"inputComplete,omitempty"`
	Input            *desktop.InputReceipt `json:"input,omitempty"`
	IdentityCapture  string                `json:"identityCapture,omitempty"`
}

func waitReceiverRelease(ctx context.Context, releaseAt time.Time) error {
	if releaseAt.IsZero() {
		return errors.New("live.receiver_release_unproven")
	}
	delay := time.Until(releaseAt)
	if delay <= 0 {
		return ctx.Err()
	}
	timer := time.NewTimer(delay)
	defer timer.Stop()
	select {
	case <-ctx.Done():
		return ctx.Err()
	case <-timer.C:
		return nil
	}
}

func parseFixedReload(record journal.WorkRecord) (fixedReloadIntent, fixedReloadObservation, error) {
	var intent fixedReloadIntent
	var observed fixedReloadObservation
	if record.Intent.Kind != "reload" || json.Unmarshal(record.Intent.Request, &intent) != nil || intent.Schema != "lycheedev.fixed-reload.v1" || intent.Release != buildinfo.Version || intent.IdentityNonce == "" || intent.Target.Client.Directory == "" || intent.Target.Window.ProcessID == 0 || intent.Target.Window.ProcessStartedAt == 0 {
		return intent, observed, errors.New("live.fixed_reload_intent_invalid")
	}
	if len(record.Observation) != 0 && string(record.Observation) != "null" {
		if json.Unmarshal(record.Observation, &observed) != nil || observed.Schema != "lycheedev.fixed-reload-observation.v1" || observed.NextStep < 0 || observed.NextStep > 6 {
			return intent, observed, errors.New("live.fixed_reload_observation_invalid")
		}
	}
	return intent, observed, nil
}

func fixedReloadManaged(ctx context.Context, intent fixedReloadIntent) error {
	status, err := delivery.InspectInstallation(ctx, delivery.AddonDirectory(intent.Target.Client.Directory), "addon")
	if err != nil {
		return err
	}
	if status.State != "managed" || status.Receipt == nil || status.Receipt.Version != intent.Release || status.Receipt.Commit != intent.Commit {
		return ErrUpgradeChanged
	}
	return nil
}

func selectFixedReloadTarget(ctx context.Context, request FixedReloadRequest) (ClientWindow, error) {
	windows, err := desktop.ListWindows(ctx)
	if err != nil {
		return ClientWindow{}, err
	}
	io := nativeIO()
	var chosen ClientWindow
	for _, window := range windows {
		if window.ProcessID != request.PID {
			continue
		}
		client, err := windowClient(ctx, window, io)
		if err != nil {
			return ClientWindow{}, err
		}
		if !sameInstallationPath(client.Directory, request.Installation) {
			continue
		}
		if chosen.Window.ProcessID != 0 {
			return ClientWindow{}, ErrCandidateAmbiguous
		}
		chosen = ClientWindow{Client: client, Window: window}
	}
	if chosen.Window.ProcessID == 0 {
		return ClientWindow{}, ErrCandidateMissing
	}
	return chosen, ConfirmClientWindow(ctx, chosen)
}

// FixedReloadClient is the sole no-session recovery path. It accepts only the
// fixed Esc×3, Enter, /reload, Enter native sequence and never sends it twice.
func FixedReloadClient(ctx context.Context, root string, request FixedReloadRequest) (Outcome, error) {
	if err := request.Validate(); err != nil {
		return Outcome{}, err
	}
	target, err := selectFixedReloadTarget(ctx, request)
	if err != nil {
		return Outcome{}, err
	}
	status, err := delivery.InspectInstallation(ctx, delivery.AddonDirectory(target.Client.Directory), "addon")
	if err != nil {
		return Outcome{}, err
	}
	if status.State != "managed" || status.Receipt == nil || status.Receipt.Version != buildinfo.Version {
		return Outcome{}, ErrUpgradeInstallation
	}
	var entropy [16]byte
	if _, err := rand.Read(entropy[:]); err != nil {
		return Outcome{}, err
	}
	intent := fixedReloadIntent{Schema: "lycheedev.fixed-reload.v1", Target: target, Release: buildinfo.Version,
		Commit: status.Receipt.Commit, IdentityNonce: hex.EncodeToString(entropy[:])}
	intent.Bindings, _ = requestedReceiverBindings(request.WakeBinding)
	if request.Session != "" {
		bound, err := ReadWindowSession(ctx, root, request.Session)
		if err != nil {
			return Outcome{}, err
		}
		if bound.Target != target {
			return Outcome{}, errors.New("live.fixed_reload_session_mismatch")
		}
		if request.WakeBinding != "" && request.WakeBinding != bound.Record.Bindings.WakeBinding {
			return Outcome{}, errors.New("live.fixed_reload_binding_mismatch")
		}
		intent.OldEpoch, intent.OldGUID, intent.OldCharacter, intent.OldRealm = bound.Ready.RuntimeEpoch, bound.Ready.GUID, bound.Ready.Character, bound.Ready.Realm
		intent.OldReportScope = bound.Ready.ReportScope
		intent.Bindings = bound.Record.Bindings
		intent.KnownBindings = true
	}
	raw, _ := json.Marshal(intent)
	key := sha256.Sum256([]byte(windowResource(target) + "\x00" + request.Request))
	digest := sha256.Sum256([]byte(target.Client.Directory + "\x00" + buildinfo.Version + "\x00" + status.Receipt.Commit + "\x00" + request.Session + "\x00" + request.WakeBinding))
	work := journal.WorkIntent{Kind: "reload", Resource: windowResource(target), Request: raw, RequestKey: "live-" + hex.EncodeToString(key[:]), RequestDigest: hex.EncodeToString(digest[:]), Goal: "cleaned"}
	record, err := vault.WriteMetadata(ctx, root, func(store *vault.Store, metadata *vault.Metadata) (journal.WorkRecord, error) {
		return journal.OpenBook(metadata).BeginWindowWork(ctx, filepath.Join(target.Client.Directory, "Interface", "AddOns"), store.Identity().WorkspaceID, work)
	})
	if err != nil {
		return finishOutcome(ctx, root, record, err)
	}
	record, err = resumeFixedReload(ctx, root, record)
	return finishOutcome(ctx, root, record, err)
}

func fixedReloadUpdate(ctx context.Context, metadata *vault.Metadata, record journal.WorkRecord, stage, status string, observed fixedReloadObservation) (journal.WorkRecord, error) {
	raw, _ := json.Marshal(observed)
	if err := journal.OpenBook(metadata).AdvanceStage(ctx, journal.StageChange{OperationID: record.OperationID, ExpectedGeneration: record.Generation,
		ExpectedStage: record.Stage, Stage: stage, Status: status, Observation: raw}); err != nil {
		return record, err
	}
	return journal.OpenBook(metadata).InspectWork(ctx, record.OperationID)
}

func resumeFixedReload(ctx context.Context, root string, record journal.WorkRecord) (journal.WorkRecord, error) {
	intent, observed, err := parseFixedReload(record)
	if err != nil {
		return record, err
	}
	if record.Stage == "cleaned" {
		return releaseCompletedFixedReload(ctx, root, record)
	}
	if record.Stage == "abandoned" {
		return record, nil
	}
	if err := ConfirmClientWindow(ctx, intent.Target); err != nil {
		return record, err
	}
	if err := fixedReloadManaged(ctx, intent); err != nil {
		return record, err
	}
	store, err := vault.OpenStore(root)
	if err != nil {
		return record, err
	}
	metadata, err := store.OpenMetadata(ctx)
	if err != nil {
		return record, err
	}
	defer metadata.Close()
	run, err := journal.OpenBook(metadata).AcquireWindowWork(ctx, filepath.Join(intent.Target.Client.Directory, "Interface", "AddOns"), store.Identity().WorkspaceID, record.OperationID)
	if err != nil {
		return record, err
	}
	defer run.Close()
	guard := func(ctx context.Context) error {
		if _, err := run.Check(ctx); err != nil {
			return err
		}
		if err := ConfirmClientWindow(ctx, intent.Target); err != nil {
			return err
		}
		return fixedReloadManaged(ctx, intent)
	}
	if record.Stage == "prepared" {
		observed = fixedReloadObservation{Schema: "lycheedev.fixed-reload-observation.v1", DismissRequested: intent.KnownBindings}
		record, err = fixedReloadUpdate(ctx, metadata, record, "reload_requested", "running", observed)
		if err != nil {
			return record, err
		}
		bindings := intent.Bindings
		if bindings.WakeBinding == "" {
			bindings = desktop.DefaultReceiverBindings()
		}
		receipt, sendErr := desktop.WithReceiverInputProfile(ctx, intent.Target.Window, bindings, guard, func(input *desktop.ReceiverInput) error {
			// A queued CLOSE is not proof the modal released. Wait beyond the
			// receiver's independent 20 second hard deadline before any Esc.
			var err error
			if observed.DismissRequested {
				if err := input.Dismiss(); err != nil {
					return err
				}
				observed.CloseQueued = true
			}
			observed.ReleaseAt = time.Now().Add(21 * time.Second).UTC()
			record, err = fixedReloadUpdate(ctx, metadata, record, "reload_requested", "running", observed)
			if err != nil {
				return err
			}
			if err := waitReceiverRelease(ctx, observed.ReleaseAt); err != nil {
				return err
			}
			observed.ReceiverReleased = true
			record, err = fixedReloadUpdate(ctx, metadata, record, "reload_requested", "running", observed)
			if err != nil {
				return err
			}
			return input.FixedReload(func(step int) error {
				if err := guard(ctx); err != nil {
					return err
				}
				observed.NextStep = step
				record, err = fixedReloadUpdate(ctx, metadata, record, "reload_requested", "running", observed)
				return err
			})
		})
		persist, cancel := context.WithTimeout(context.WithoutCancel(ctx), 5*time.Second)
		defer cancel()
		latest, inspectErr := journal.OpenBook(metadata).InspectWork(persist, record.OperationID)
		if inspectErr == nil {
			record = latest
		}
		if observed.ReceiverReleased && observed.NextStep == 6 && sendErr == nil {
			observed.InputComplete = true
			observed.Input = &receipt
			record, err = fixedReloadUpdate(persist, metadata, record, "reload_requested", "running", observed)
			if err != nil {
				return record, err
			}
		}
		if sendErr != nil {
			return record, &BootstrapPendingError{ID: record.OperationID, Cause: sendErr}
		}
	}
	if !observed.InputComplete {
		return record, &BootstrapPendingError{ID: record.OperationID, Cause: errors.New("fixed reload input progress unknown; no replay")}
	}
	return observeFixedReload(ctx, root, metadata, run, record, intent, observed)
}

func observeFixedReload(ctx context.Context, root string, metadata *vault.Metadata, run *journal.WindowRun, record journal.WorkRecord, intent fixedReloadIntent, observed fixedReloadObservation) (journal.WorkRecord, error) {
	guard := func(ctx context.Context) error {
		if _, err := run.Check(ctx); err != nil {
			return err
		}
		if err := ConfirmClientWindow(ctx, intent.Target); err != nil {
			return err
		}
		return fixedReloadManaged(ctx, intent)
	}
	// The fixed sequence has already been sent once. Identity is a separate
	// read-only business query through the receiver; receiver attempt evidence
	// prevents repeating an uncertain query after process loss.
	progress := func(ctx context.Context, phase string, stage bridge.ReceiverStage, number int, signal bridge.Signal) error {
		return saveReceiverAttempt(ctx, metadata, record.OperationID, phase, stage, number, signal)
	}
	prior, priorErr := receiverAttemptForWork(ctx, metadata, record.OperationID, "identify", "-", intent.IdentityNonce)
	if priorErr != nil && !errors.Is(priorErr, vault.ErrMissingRecord) {
		return record, priorErr
	}
	if priorErr == nil && prior.Phase != "commit_requested" && prior.Phase != "accepted" && prior.Phase != "dismissed" {
		return record, &BootstrapPendingError{ID: record.OperationID, Cause: errors.New("identity input progress unknown; no replay")}
	}
	if errors.Is(priorErr, vault.ErrMissingRecord) {
		_, err := sendReceiverInput(withReceiverBindings(ctx, intent.Bindings), intent.Target, image.Rectangle{}, bridge.SignalIdentity{},
			func(context.Context) (string, error) { return "/dev bridge identify " + intent.IdentityNonce, nil }, guard, progress)
		var inspectErr error
		prior, inspectErr = receiverAttemptForWork(ctx, metadata, record.OperationID, "identify", "-", intent.IdentityNonce)
		if inspectErr != nil {
			return record, errors.Join(err, inspectErr)
		}
		if err != nil && prior.Phase != "commit_requested" && prior.Phase != "accepted" && prior.Phase != "dismissed" {
			return record, err
		}
	}
	if err := validateFixedReloadEpoch(intent, prior); err != nil {
		return record, err
	}
	frames, err := desktop.CaptureFrames(ctx, intent.Target.Window, image.Rectangle{})
	if err != nil {
		return record, err
	}
	defer frames.Close()
	reader := bridge.ObserveSignals(frames)
	wait, cancel := context.WithTimeout(ctx, 20*time.Second)
	defer cancel()
	signal, err := reader.DiscoverIdentity(wait, bridge.SignalExpectation{Kind: "identity", Release: intent.Release,
		ProbeNonce: intent.IdentityNonce, Product: intent.Target.Client.Product, Build: intent.Target.Client.FullBuild})
	if err != nil {
		return record, err
	}
	if intent.OldGUID != "" && signal.GUID != intent.OldGUID || intent.OldCharacter != "" && signal.Character != intent.OldCharacter || intent.OldRealm != "" && signal.Realm != intent.OldRealm {
		return record, errors.New("live.fixed_reload_identity_mismatch")
	}
	store, err := vault.OpenStore(root)
	if err != nil {
		return record, err
	}
	raw, _ := json.Marshal(signal)
	capture, err := evidence.OpenArchive(store, metadata).CommitCapture(ctx, evidence.CaptureDraft{Reader: bytes.NewReader(raw), MaxBytes: 4096, MediaType: "application/json", Complete: true,
		Provenance: evidence.Provenance{Kind: "decoded-fixed-reload", Locator: intent.IdentityNonce, OperationID: record.OperationID, DataBuild: signal.Build}})
	if err != nil {
		return record, err
	}
	observed.IdentityCapture = capture.ID
	record, err = fixedReloadUpdate(ctx, metadata, record, "cleaned", "completed", observed)
	if err != nil {
		return record, err
	}
	if err := run.Close(); err != nil {
		return record, err
	}
	if err := journal.OpenBook(metadata).RetireWindowWork(ctx, filepath.Join(intent.Target.Client.Directory, "Interface", "AddOns"), store.Identity().WorkspaceID, record.OperationID); err != nil {
		return record, err
	}
	return record, nil
}

func validateFixedReloadEpoch(intent fixedReloadIntent, prior receiverAttempt) error {
	if prior.Epoch == 0 || intent.OldReportScope != "" && prior.ReportScope != intent.OldReportScope {
		return errors.New("live.fixed_reload_epoch_unproven")
	}
	// Character state begins a separate counter domain. Its explicit capability
	// proves activation of the new storage protocol; never compare it against
	// the retired account counter, and never accept a downgrade in scope.
	if intent.OldReportScope == "" && prior.ReportScope == "character-v1" {
		return nil
	}
	if intent.OldEpoch != 0 && prior.Epoch <= intent.OldEpoch {
		return errors.New("live.fixed_reload_epoch_unproven")
	}
	return nil
}

func releaseCompletedFixedReload(ctx context.Context, root string, record journal.WorkRecord) (journal.WorkRecord, error) {
	intent, _, err := parseFixedReload(record)
	if err != nil || record.Stage != "cleaned" || record.Status != "completed" {
		return record, errors.Join(err, journal.ErrTransition)
	}
	_, err = vault.WriteMetadata(ctx, root, func(store *vault.Store, metadata *vault.Metadata) (struct{}, error) {
		return struct{}{}, journal.OpenBook(metadata).RetireWindowWork(ctx, filepath.Join(intent.Target.Client.Directory, "Interface", "AddOns"), store.Identity().WorkspaceID, record.OperationID)
	})
	return record, err
}

// Explicit abandonment releases disk ownership after unknown native progress.
// It never claims the reload, identity query or runtime activation succeeded.
func abandonFixedReload(ctx context.Context, root string, record journal.WorkRecord) (journal.WorkRecord, error) {
	intent, observed, err := parseFixedReload(record)
	if err != nil {
		return record, err
	}
	if record.Stage != "prepared" && record.Stage != "reload_requested" && record.Stage != "abandoning" && record.Stage != "abandoned" {
		return record, journal.ErrTransition
	}
	return vault.WriteMetadata(ctx, root, func(store *vault.Store, metadata *vault.Metadata) (journal.WorkRecord, error) {
		book := journal.OpenBook(metadata)
		current, err := book.InspectWork(ctx, record.OperationID)
		if err != nil {
			return current, err
		}
		if current.Stage == "prepared" || current.Stage == "reload_requested" {
			if observed.Schema == "" {
				observed.Schema = "lycheedev.fixed-reload-observation.v1"
			}
			current, err = fixedReloadUpdate(ctx, metadata, current, "abandoning", "running", observed)
			if err != nil {
				return current, err
			}
		}
		if current.Stage == "abandoning" {
			current, err = fixedReloadUpdate(ctx, metadata, current, "abandoned", "abandoned", observed)
			if err != nil {
				return current, err
			}
		}
		if err := book.RetireWindowWork(ctx, filepath.Join(intent.Target.Client.Directory, "Interface", "AddOns"), store.Identity().WorkspaceID, current.OperationID); err != nil {
			return current, err
		}
		return current, nil
	})
}

func fixedReloadOutcome(ctx context.Context, archive *evidence.Archive, record journal.WorkRecord, result Outcome) (Outcome, error) {
	intent, observed, err := parseFixedReload(record)
	if err != nil {
		return result, err
	}
	if record.Stage != "cleaned" || record.Status != "completed" {
		return result, nil
	}
	ref, raw, err := archive.FetchCapture(ctx, observed.IdentityCapture, 4096)
	if err != nil {
		return result, err
	}
	if ref.Provenance.Kind != "decoded-fixed-reload" || ref.Provenance.Locator != intent.IdentityNonce || ref.Provenance.OperationID != record.OperationID || !ref.Complete || ref.Truncated {
		return result, errors.New("live.fixed_reload_evidence_mismatch")
	}
	signal, err := bridge.ParseSignal(raw)
	if err != nil {
		return result, err
	}
	if signal.Kind != "identity" || signal.ProbeNonce != intent.IdentityNonce || signal.Release != intent.Release || signal.Product != intent.Target.Client.Product || signal.Build != intent.Target.Client.FullBuild {
		return result, errors.New("live.fixed_reload_evidence_mismatch")
	}
	result.Complete = true
	result.RuntimeCapture = ref.ID
	return result, nil
}
