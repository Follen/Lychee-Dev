package live

import (
	"context"
	"crypto/rand"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"image"
	"path/filepath"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/buildinfo"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/vault"
)

type BootstrapReceiverAttempt struct {
	Schema             string                   `json:"schema"`
	ID                 string                   `json:"id"`
	Target             ClientWindow             `json:"target"`
	Action             string                   `json:"action"`
	RequestID          string                   `json:"requestId"`
	Arg                string                   `json:"arg"`
	Phase              string                   `json:"phase"`
	Claimed            bool                     `json:"claimed,omitempty"`
	Receiver           receiverAttempt          `json:"receiver"`
	History            []receiverAttempt        `json:"history,omitempty"`
	Bindings           desktop.ReceiverBindings `json:"bindings,omitempty"`
	ActorGUID          string                   `json:"actorGuid,omitempty"`
	ActorCharacter     string                   `json:"actorCharacter,omitempty"`
	ActorRealm         string                   `json:"actorRealm,omitempty"`
	PriorSessionNonce  string                   `json:"priorSessionNonce,omitempty"`
	PriorReadyNonce    string                   `json:"priorReadyNonce,omitempty"`
	PriorReadyEpoch    uint64                   `json:"priorReadyEpoch,omitempty"`
	PriorReadySequence uint64                   `json:"priorReadySequence,omitempty"`
	Business           *bridge.Signal           `json:"business,omitempty"`
	CreatedAt          time.Time                `json:"createdAt"`
	UpdatedAt          time.Time                `json:"updatedAt"`
}

type BootstrapPendingError struct {
	ID    string
	Cause error
}

func (e *BootstrapPendingError) Error() string {
	return "live.bootstrap_pending: " + e.ID + ": " + e.Cause.Error()
}
func (e *BootstrapPendingError) Unwrap() error { return e.Cause }

type bootstrapCurrent struct {
	ID string `json:"id"`
}

type bootstrapPriorReadyKey struct{}

func bootstrapOwner(workspace string, attempt BootstrapReceiverAttempt) journal.WindowOwner {
	raw, _ := json.Marshal(struct {
		ID                   string
		Target               ClientWindow
		Action, Request, Arg string
	}{attempt.ID, attempt.Target, attempt.Action, attempt.RequestID, attempt.Arg})
	return journal.WindowOwner{Schema: "lycheedev.window-owner.v1", WorkspaceID: workspace, Resource: windowResource(attempt.Target), OperationID: attempt.ID, IntentSHA256: fmt.Sprintf("%x", sha256.Sum256(raw))}
}

func retireBootstrapClaim(ctx context.Context, root string, attempt BootstrapReceiverAttempt) error {
	if !attempt.Claimed {
		return nil
	}
	if attempt.Phase != "confirmed" && attempt.Phase != "abandoned" {
		return journal.ErrTransition
	}
	store, err := vault.OpenStore(root)
	if err != nil {
		return err
	}
	return journal.RetireBootstrapWindow(ctx, filepath.Join(attempt.Target.Client.Directory, "Interface", "AddOns"), bootstrapOwner(store.Identity().WorkspaceID, attempt))
}

func withBootstrapPriorReady(ctx context.Context, ready bridge.Signal) context.Context {
	if ready.SessionNonce == "" {
		return ctx
	}
	return context.WithValue(ctx, bootstrapPriorReadyKey{}, ready)
}

func bootstrapCurrentKey(target ClientWindow, action string) string {
	digest := sha256.Sum256([]byte(windowResource(target) + "\x00" + action))
	return "bootstrap/current/" + hex.EncodeToString(digest[:])
}

func bootstrapWindowKey(target ClientWindow) string {
	digest := sha256.Sum256([]byte(windowResource(target)))
	return "bootstrap/window/" + hex.EncodeToString(digest[:])
}

func BeginBootstrapReceiver(ctx context.Context, root string, target ClientWindow, command string) (BootstrapReceiverAttempt, error) {
	action, request, arg, err := parseBridgeCommand(command)
	if err != nil {
		return BootstrapReceiverAttempt{}, err
	}
	if action != "identify" && action != "connect" && action != "reset" && action != "hide" {
		return BootstrapReceiverAttempt{}, errors.New("live.bootstrap_action_invalid")
	}
	var nonce [16]byte
	if _, err := rand.Read(nonce[:]); err != nil {
		return BootstrapReceiverAttempt{}, err
	}
	now := time.Now().UTC()
	attempt := BootstrapReceiverAttempt{Schema: "lycheedev.bootstrap-receiver.v1", ID: "BTP-" + hex.EncodeToString(nonce[:]),
		Target: target, Action: action, RequestID: request, Arg: arg, Phase: "prepared", Claimed: true, CreatedAt: now, UpdatedAt: now}
	return vault.WriteMetadata(ctx, root, func(store *vault.Store, metadata *vault.Metadata) (BootstrapReceiverAttempt, error) {
		key := bootstrapCurrentKey(target, action)
		windowKey := bootstrapWindowKey(target)
		windowDoc, err := metadata.ReadDocument(ctx, windowKey)
		if err != nil && !errors.Is(err, vault.ErrMissingRecord) {
			return attempt, err
		}
		if err == nil {
			var current bootstrapCurrent
			if json.Unmarshal(windowDoc.Value, &current) != nil || current.ID == "" {
				return attempt, errors.New("live.bootstrap_window_corrupt")
			}
			previous, err := readBootstrapReceiver(ctx, metadata, current.ID)
			if err != nil {
				return attempt, err
			}
			if previous.Phase != "confirmed" && previous.Phase != "abandoned" {
				return previous, &BootstrapPendingError{ID: previous.ID, Cause: errors.New("prior window input or receipt unresolved")}
			}
			if err := retireBootstrapClaim(ctx, root, previous); err != nil {
				return previous, err
			}
		}
		currentDoc, err := metadata.ReadDocument(ctx, key)
		if err != nil && !errors.Is(err, vault.ErrMissingRecord) {
			return attempt, err
		}
		if err == nil {
			var current bootstrapCurrent
			if json.Unmarshal(currentDoc.Value, &current) != nil || current.ID == "" {
				return attempt, errors.New("live.bootstrap_current_corrupt")
			}
			previous, err := readBootstrapReceiver(ctx, metadata, current.ID)
			if err != nil {
				return attempt, err
			}
			if previous.Phase != "confirmed" && previous.Phase != "abandoned" {
				return attempt, &BootstrapPendingError{ID: previous.ID, Cause: errors.New("prior input or business receipt unresolved")}
			}
		}
		raw, _ := json.Marshal(attempt)
		pointer, _ := json.Marshal(bootstrapCurrent{ID: attempt.ID})
		persisted := false
		err = journal.BeginBootstrapWindow(ctx, filepath.Join(target.Client.Directory, "Interface", "AddOns"), bootstrapOwner(store.Identity().WorkspaceID, attempt), func() error {
			err := metadata.CommitDocuments(ctx, vault.Mutation{Key: "bootstrap/attempt/" + attempt.ID, Value: raw},
				vault.Mutation{Key: key, ExpectedGeneration: currentDoc.Generation, Value: pointer},
				vault.Mutation{Key: windowKey, ExpectedGeneration: windowDoc.Generation, Value: pointer})
			persisted = err == nil
			return err
		})
		if err != nil {
			if !persisted {
				return BootstrapReceiverAttempt{}, err
			}
			return attempt, err
		}
		return attempt, nil
	})
}

// AbandonBootstrapReceiver is an explicit host-side escape for a first-contact
// attempt whose native or business result cannot be resolved. It preserves the
// full attempt and never sends game input or infers business completion.
func AbandonBootstrapReceiver(ctx context.Context, root, id string) (BootstrapReceiverAttempt, error) {
	prior, err := InspectBootstrapReceiver(ctx, root, id)
	if err != nil {
		return prior, err
	}
	var driver *vault.Lease
	if prior.Claimed && prior.Phase != "abandoned" {
		store, err := vault.OpenStore(root)
		if err != nil {
			return prior, err
		}
		parent := filepath.Join(prior.Target.Client.Directory, "Interface", "AddOns")
		_, busy, err := journal.InspectWindowOwner(ctx, parent, windowResource(prior.Target))
		if err != nil {
			return prior, err
		}
		if busy {
			driver, err = journal.LockBootstrapWindow(ctx, parent, bootstrapOwner(store.Identity().WorkspaceID, prior))
			if err != nil {
				return prior, err
			}
			defer driver.Close()
		} else if prior.Phase != "prepared" {
			return prior, errors.New("live.bootstrap_claim_missing")
		}
	}
	attempt, err := vault.WriteMetadata(ctx, root, func(_ *vault.Store, metadata *vault.Metadata) (BootstrapReceiverAttempt, error) {
		key := "bootstrap/attempt/" + id
		doc, err := metadata.ReadDocument(ctx, key)
		if err != nil {
			return BootstrapReceiverAttempt{}, err
		}
		var attempt BootstrapReceiverAttempt
		if json.Unmarshal(doc.Value, &attempt) != nil || attempt.Schema != "lycheedev.bootstrap-receiver.v1" || attempt.ID != id {
			return attempt, errors.New("live.bootstrap_attempt_corrupt")
		}
		if attempt.Phase == "abandoned" {
			return attempt, nil
		}
		if attempt.Phase == "confirmed" {
			return attempt, errors.New("live.bootstrap_already_confirmed")
		}
		currentDoc, err := metadata.ReadDocument(ctx, bootstrapCurrentKey(attempt.Target, attempt.Action))
		if err != nil {
			return attempt, err
		}
		var current bootstrapCurrent
		if json.Unmarshal(currentDoc.Value, &current) != nil || current.ID != id {
			return attempt, errors.New("live.bootstrap_current_mismatch")
		}
		windowDoc, err := metadata.ReadDocument(ctx, bootstrapWindowKey(attempt.Target))
		if err != nil {
			return attempt, err
		}
		if json.Unmarshal(windowDoc.Value, &current) != nil || current.ID != id {
			return attempt, errors.New("live.bootstrap_window_mismatch")
		}
		attempt.Phase = "abandoned"
		attempt.UpdatedAt = time.Now().UTC()
		raw, _ := json.Marshal(attempt)
		if err := metadata.CommitDocuments(ctx, vault.Mutation{Key: key, ExpectedGeneration: doc.Generation, Value: raw}); err != nil {
			return attempt, err
		}
		return attempt, nil
	})
	if driver != nil {
		err = errors.Join(err, driver.Close())
	}
	if err == nil {
		err = retireBootstrapClaim(ctx, root, attempt)
	}
	return attempt, err
}

func readBootstrapReceiver(ctx context.Context, metadata *vault.Metadata, id string) (BootstrapReceiverAttempt, error) {
	doc, err := metadata.ReadDocument(ctx, "bootstrap/attempt/"+id)
	if err != nil {
		return BootstrapReceiverAttempt{}, err
	}
	var attempt BootstrapReceiverAttempt
	if json.Unmarshal(doc.Value, &attempt) != nil || attempt.Schema != "lycheedev.bootstrap-receiver.v1" || attempt.ID != id {
		return attempt, errors.New("live.bootstrap_attempt_corrupt")
	}
	return attempt, nil
}

func InspectBootstrapReceiver(ctx context.Context, root, id string) (BootstrapReceiverAttempt, error) {
	return vault.ReadWorkspace(ctx, root, func(_ *vault.Store, metadata *vault.Metadata) (BootstrapReceiverAttempt, error) {
		return readBootstrapReceiver(ctx, metadata, id)
	})
}

func bootstrapBusinessMayHaveExecuted(ctx context.Context, root, id string) bool {
	if id == "" {
		return false
	}
	attempt, err := InspectBootstrapReceiver(ctx, root, id)
	if err != nil {
		return false
	}
	switch attempt.Receiver.Phase {
	case "commit_requested", "accepted", "dismissed":
		return true
	default:
		return false
	}
}

// ResumeBootstrapReceiver observes the original business receipt without
// replaying input. Exact business evidence confirms the attempt; input UI
// release is owned by the receiver and requires no host cleanup transaction.
func ResumeBootstrapReceiver(ctx context.Context, root, id string) (BootstrapReceiverAttempt, error) {
	attempt, err := InspectBootstrapReceiver(ctx, root, id)
	if err != nil {
		return attempt, err
	}
	if attempt.Phase == "confirmed" || attempt.Phase == "abandoned" {
		return attempt, retireBootstrapClaim(ctx, root, attempt)
	}
	if bootstrapCanRestage(attempt) {
		command := "/dev bridge " + attempt.Action + " " + attempt.Arg
		if attempt.Action == "connect" {
			command = "/dev connect"
		}
		if attempt.Action == "hide" {
			command = "/dev bridge hide"
		}
		ctx = withReceiverBindings(ctx, attempt.Bindings)
		if _, _, err := sendBootstrapAttempt(ctx, root, attempt, image.Rectangle{}, command); err != nil {
			if latest, readErr := InspectBootstrapReceiver(context.WithoutCancel(ctx), root, id); readErr == nil {
				attempt = latest
			}
			return attempt, err
		}
		attempt, err = InspectBootstrapReceiver(ctx, root, id)
		if err != nil {
			return attempt, err
		}
	}
	if attempt.Receiver.Phase != "commit_requested" && attempt.Receiver.Phase != "accepted" && attempt.Receiver.Phase != "dismissed" {
		return attempt, &BootstrapPendingError{ID: id, Cause: errors.New("receiver acceptance unresolved")}
	}
	if attempt.Action == "connect" && attempt.PriorSessionNonce != "" && attempt.PriorReadyNonce == "" {
		prior, err := recoverBootstrapPriorReady(ctx, root, attempt)
		if err != nil {
			return attempt, &BootstrapPendingError{ID: id, Cause: err}
		}
		if err := updateBootstrapReceiver(ctx, root, id, func(current *BootstrapReceiverAttempt) error {
			if current.Phase == "confirmed" || current.Phase == "abandoned" || current.PriorSessionNonce != prior.SessionNonce {
				return errors.New("live.bootstrap_attempt_conflict")
			}
			current.PriorReadyNonce, current.PriorReadyEpoch, current.PriorReadySequence = prior.SessionNonce, prior.RuntimeEpoch, prior.Sequence
			return nil
		}); err != nil {
			return attempt, err
		}
		attempt.PriorReadyNonce, attempt.PriorReadyEpoch, attempt.PriorReadySequence = prior.SessionNonce, prior.RuntimeEpoch, prior.Sequence
	}
	if err := ConfirmClientWindow(ctx, attempt.Target); err != nil {
		return attempt, err
	}
	frames, err := desktop.CaptureFrames(ctx, attempt.Target.Window, image.Rectangle{})
	if err != nil {
		return attempt, err
	}
	defer frames.Close()
	reader := bridge.ObserveSignals(frames)
	wait, cancel := context.WithTimeout(ctx, 15*time.Second)
	defer cancel()
	expected := bridge.SignalExpectation{Release: buildinfo.Version, Product: attempt.Target.Client.Product, Build: attempt.Target.Client.FullBuild}
	var signal bridge.Signal
	switch attempt.Action {
	case "identify":
		expected.Kind, expected.ProbeNonce = "identity", attempt.Arg
		signal, err = reader.DiscoverIdentity(wait, expected)
	case "reset":
		expected.Kind, expected.ProbeNonce = "reset", attempt.Arg
		signal, err = reader.DiscoverReset(wait, expected)
	case "connect":
		expected.Kind, expected.RequireInputReady = "ready", true
		signal, err = reader.DiscoverReady(wait, expected)
	case "hide":
		if attempt.Receiver.Phase != "commit_requested" && attempt.Receiver.Phase != "accepted" && attempt.Receiver.Phase != "dismissed" {
			return attempt, &BootstrapPendingError{ID: id, Cause: errors.New("hide acceptance unconfirmed")}
		}
		if err := observeBootstrapHideCleared(wait, frames); err != nil {
			return attempt, &BootstrapPendingError{ID: id, Cause: err}
		}
		if err := updateBootstrapReceiver(ctx, root, id, func(current *BootstrapReceiverAttempt) error {
			if current.Action != "hide" || (current.Phase != "commit_requested" && current.Phase != "accepted" && current.Phase != "dismissed") ||
				(current.Receiver.Phase != "commit_requested" && current.Receiver.Phase != "accepted" && current.Receiver.Phase != "dismissed") {
				return errors.New("live.bootstrap_hide_mismatch")
			}
			current.Phase = "confirmed"
			return nil
		}); err != nil {
			return attempt, err
		}
		return InspectBootstrapReceiver(ctx, root, id)
	default:
		return attempt, errors.New("live.bootstrap_action_invalid")
	}
	if err != nil {
		return attempt, &BootstrapPendingError{ID: id, Cause: err}
	}
	if err := confirmBootstrapReceiver(ctx, root, id, signal); err != nil {
		return attempt, err
	}
	return InspectBootstrapReceiver(ctx, root, id)
}

// Older pending reconnects did not save their retained ready baseline. Recover
// it only from a verified session capture for the exact window, actor, nonce,
// and runtime epoch. A bounded scan fails closed rather than guessing freshness.
func recoverBootstrapPriorReady(ctx context.Context, root string, attempt BootstrapReceiverAttempt) (bridge.Signal, error) {
	return vault.ReadWorkspace(ctx, root, func(store *vault.Store, metadata *vault.Metadata) (bridge.Signal, error) {
		var best bridge.Signal
		after := ""
		for seen := 0; seen < 512; {
			page, err := metadata.ListDocuments(ctx, "session/", after, 64)
			if err != nil {
				return bridge.Signal{}, err
			}
			if len(page) == 0 {
				if best.SessionNonce == "" {
					return best, errors.New("live.bootstrap_prior_ready_unavailable")
				}
				return best, nil
			}
			for _, doc := range page {
				var record SessionRecord
				if json.Unmarshal(doc.Value, &record) != nil || record.ID == "" {
					return bridge.Signal{}, errors.New("live.bootstrap_prior_session_corrupt")
				}
				// The receiver-era session record always saves its effective
				// bindings. Older records lack them and cannot be an exact prior
				// session for this receiver attempt; do not try to reinterpret
				// their historical record IDs with the current record schema.
				if record.Bindings.WakeBinding == "" {
					continue
				}
				session, err := readSessionEvidence(ctx, store, metadata, record)
				if err != nil {
					return bridge.Signal{}, err
				}
				ready := session.Ready
				if session.Target == attempt.Target && ready.SessionNonce == attempt.PriorSessionNonce && ready.RuntimeEpoch == attempt.Receiver.Epoch &&
					ready.GUID == attempt.ActorGUID && ready.Character == attempt.ActorCharacter && ready.Realm == attempt.ActorRealm && ready.Sequence > best.Sequence {
					best = ready
				}
			}
			seen += len(page)
			after = page[len(page)-1].Key
			if len(page) < 64 {
				if best.SessionNonce == "" {
					return best, errors.New("live.bootstrap_prior_ready_unavailable")
				}
				return best, nil
			}
		}
		return bridge.Signal{}, errors.New("live.bootstrap_prior_session_limit")
	})
}

func observeBootstrapHideCleared(ctx context.Context, frames sessionFrames) error {
	clear := 0
	for clear < 2 {
		frame, err := frames.Next(ctx)
		if err != nil {
			return err
		}
		if frame == nil || frame.NRGBA == nil {
			clear = 0
			continue
		}
		symbols, err := desktop.DecodeSymbols(frame.NRGBA)
		if err != nil || len(symbols) != 0 || frameHint(frame) != captureFrame {
			clear = 0
			continue
		}
		clear++
	}
	return nil
}

func updateBootstrapReceiver(ctx context.Context, root, id string, update func(*BootstrapReceiverAttempt) error) error {
	var terminal *BootstrapReceiverAttempt
	_, err := vault.WriteMetadata(ctx, root, func(_ *vault.Store, metadata *vault.Metadata) (struct{}, error) {
		key := "bootstrap/attempt/" + id
		doc, err := metadata.ReadDocument(ctx, key)
		if err != nil {
			return struct{}{}, err
		}
		var attempt BootstrapReceiverAttempt
		if json.Unmarshal(doc.Value, &attempt) != nil || attempt.Schema != "lycheedev.bootstrap-receiver.v1" || attempt.ID != id {
			return struct{}{}, errors.New("live.bootstrap_attempt_corrupt")
		}
		if err := update(&attempt); err != nil {
			return struct{}{}, err
		}
		attempt.UpdatedAt = time.Now().UTC()
		raw, _ := json.Marshal(attempt)
		err = metadata.CommitDocuments(ctx, vault.Mutation{Key: key, ExpectedGeneration: doc.Generation, Value: raw})
		if err == nil && (attempt.Phase == "confirmed" || attempt.Phase == "abandoned") {
			terminal = &attempt
		}
		return struct{}{}, err
	})
	if err == nil && terminal != nil {
		err = retireBootstrapClaim(ctx, root, *terminal)
	}
	return err
}

func confirmBootstrapReceiver(ctx context.Context, root, id string, signal bridge.Signal) error {
	return updateBootstrapReceiver(ctx, root, id, func(attempt *BootstrapReceiverAttempt) error {
		if (attempt.Phase != "commit_requested" && attempt.Phase != "accepted" && attempt.Phase != "dismissed") || (attempt.Receiver.Phase != "commit_requested" && attempt.Receiver.Phase != "accepted" && attempt.Receiver.Phase != "dismissed") {
			return errors.New("live.bootstrap_receipt_mismatch.phase")
		}
		if signal.Release == "" {
			return errors.New("live.bootstrap_receipt_mismatch.release")
		}
		if signal.Product != attempt.Target.Client.Product {
			return errors.New("live.bootstrap_receipt_mismatch.product")
		}
		if signal.Build != attempt.Target.Client.FullBuild {
			return errors.New("live.bootstrap_receipt_mismatch.build")
		}
		switch attempt.Action {
		case "identify", "reset":
			kind := attempt.Action
			if kind == "identify" {
				kind = "identity"
			}
			if signal.Kind != kind {
				return errors.New("live.bootstrap_receipt_mismatch.kind")
			}
			if signal.ProbeNonce != attempt.Arg {
				return errors.New("live.bootstrap_receipt_mismatch.probe_nonce")
			}
		case "connect":
			if signal.Kind != "ready" || !signal.InputReady || signal.SessionNonce == "" || attempt.ActorGUID == "" || signal.GUID != attempt.ActorGUID || signal.Character != attempt.ActorCharacter || signal.Realm != attempt.ActorRealm || signal.RuntimeEpoch < attempt.Receiver.Epoch {
				return errors.New("live.bootstrap_receipt_mismatch.ready")
			}
			if attempt.PriorSessionNonce != "" && signal.SessionNonce == attempt.PriorSessionNonce {
				if attempt.PriorReadyNonce != signal.SessionNonce || attempt.PriorReadyEpoch != signal.RuntimeEpoch || signal.Sequence <= attempt.PriorReadySequence {
					return errors.New("live.bootstrap_receipt_mismatch.prior_ready")
				}
			}
		}
		attempt.Business = &signal
		attempt.Phase = "confirmed"
		return nil
	})
}

func sendBootstrapReceiver(ctx context.Context, root string, target ClientWindow, region image.Rectangle, command string) (desktop.InputReceipt, string, error) {
	if root == "" {
		return desktop.InputReceipt{}, "", errors.New("live.bootstrap_workspace_missing")
	}
	if err := ConfirmClientWindow(ctx, target); err != nil {
		return desktop.InputReceipt{}, "", err
	}
	attempt, err := BeginBootstrapReceiver(ctx, root, target, command)
	if err != nil {
		return desktop.InputReceipt{}, attempt.ID, err
	}
	return sendBootstrapAttempt(ctx, root, attempt, region, command)
}

// Before commit_requested, the durable progress fence proves no business key
// was ever submitted. Re-handshake the same intent under the same window claim;
// preserve each prior attempt and bound retries across process restarts.
func bootstrapCanRestage(a BootstrapReceiverAttempt) bool {
	if a.Receiver.Attempt < 1 || a.Receiver.Attempt >= 3 || a.Phase != a.Receiver.Phase {
		return false
	}
	switch a.Phase {
	case "wake_requested", "stage_requested", "submit_requested", "rejected":
		return true
	}
	return false
}

func sendBootstrapAttempt(ctx context.Context, root string, attempt BootstrapReceiverAttempt, region image.Rectangle, command string) (desktop.InputReceipt, string, error) {
	target := attempt.Target
	if err := ConfirmClientWindow(ctx, target); err != nil {
		return desktop.InputReceipt{}, attempt.ID, err
	}
	store, err := vault.OpenStore(root)
	if err != nil {
		return desktop.InputReceipt{}, attempt.ID, err
	}
	wantOwner := bootstrapOwner(store.Identity().WorkspaceID, attempt)
	driver, err := journal.LockBootstrapWindow(ctx, filepath.Join(target.Client.Directory, "Interface", "AddOns"), wantOwner)
	if err != nil {
		return desktop.InputReceipt{}, attempt.ID, err
	}
	defer driver.Close()
	if prior, ok := ctx.Value(bootstrapPriorReadyKey{}).(bridge.Signal); ok && attempt.Action == "connect" {
		if prior.Kind != "ready" || prior.SessionNonce == "" || prior.RuntimeEpoch == 0 || prior.Product != target.Client.Product || prior.Build != target.Client.FullBuild {
			return desktop.InputReceipt{}, attempt.ID, errors.New("live.bootstrap_prior_ready_invalid")
		}
		if err := updateBootstrapReceiver(ctx, root, attempt.ID, func(current *BootstrapReceiverAttempt) error {
			if current.Phase != "prepared" {
				return errors.New("live.bootstrap_attempt_conflict")
			}
			current.PriorReadyNonce = prior.SessionNonce
			current.PriorReadyEpoch = prior.RuntimeEpoch
			current.PriorReadySequence = prior.Sequence
			return nil
		}); err != nil {
			return desktop.InputReceipt{}, attempt.ID, err
		}
	}
	guard := func(ctx context.Context) error {
		if err := ConfirmClientWindow(ctx, target); err != nil {
			return err
		}
		active, err := InspectBootstrapReceiver(ctx, root, attempt.ID)
		if err != nil {
			return err
		}
		if active.Phase == "abandoned" || active.Phase == "confirmed" || active.Target != target || active.Action != attempt.Action || active.RequestID != attempt.RequestID || active.Arg != attempt.Arg {
			return errors.New("live.bootstrap_attempt_inactive")
		}
		store, err := vault.OpenStore(root)
		if err != nil {
			return err
		}
		owner, occupied, err := journal.InspectWindowOwner(ctx, filepath.Join(target.Client.Directory, "Interface", "AddOns"), windowResource(target))
		if err != nil {
			return err
		}
		if !occupied {
			return errors.New("live.bootstrap_claim_missing")
		}
		if owner != wantOwner {
			return &journal.WindowOccupied{Owner: owner, Foreign: owner.WorkspaceID != store.Identity().WorkspaceID}
		}
		return nil
	}
	progress := func(ctx context.Context, phase string, stage bridge.ReceiverStage, number int, receipt bridge.Signal) error {
		persist, cancel := context.WithTimeout(context.WithoutCancel(ctx), 5*time.Second)
		defer cancel()
		return updateBootstrapReceiver(persist, root, attempt.ID, func(current *BootstrapReceiverAttempt) error {
			if current.Phase == "confirmed" || current.Phase == "abandoned" || current.Action != stage.Action || current.RequestID != stage.RequestID || current.Arg != stage.Arg || current.Target != target {
				return errors.New("live.bootstrap_attempt_conflict")
			}
			if phase == "wake_requested" {
				if current.Phase != "prepared" && !bootstrapCanRestage(*current) {
					return errors.New("live.bootstrap_retry_unconfirmed")
				}
				if current.Receiver.Attempt > 0 {
					current.History = append(current.History, current.Receiver)
				}
			}
			ordinal := attempt.Receiver.Attempt + number + 1
			if ordinal > 3 {
				return errors.New("live.receiver_retry_limit")
			}
			current.Phase = phase
			current.Receiver = receiverAttempt{Schema: "lycheedev.receiver-attempt.v1", OperationID: attempt.ID,
				Action: stage.Action, RequestID: stage.RequestID, Arg: stage.Arg, Attempt: ordinal, AttemptID: stage.AttemptID,
				Nonce: stage.ReceiverNonce, Epoch: stage.RuntimeEpoch, Phase: phase, ReceiptKind: receipt.Kind,
				Sequence: receipt.Sequence, ErrorCode: receipt.ErrorCode, UpdatedAt: time.Now().UTC()}
			if receipt.Kind == "receiver_ready" {
				current.Bindings = desktop.ReceiverBindings{WakeBinding: receipt.WakeBinding, SubmitBinding: receipt.SubmitBinding, CloseBinding: receipt.CloseBinding}
				current.ActorGUID, current.ActorCharacter, current.ActorRealm = receipt.GUID, receipt.Character, receipt.Realm
				current.PriorSessionNonce = receipt.SessionNonce
				if receipt.SessionNonce != "" {
					if current.PriorReadyNonce == receipt.SessionNonce && current.PriorReadyEpoch == receipt.PriorSessionEpoch && receipt.PriorSessionSequence < current.PriorReadySequence {
						return errors.New("live.bootstrap_prior_ready_regressed")
					}
					current.PriorReadyNonce, current.PriorReadyEpoch, current.PriorReadySequence = receipt.SessionNonce, receipt.PriorSessionEpoch, receipt.PriorSessionSequence
				}
			}
			return nil
		})
	}
	receipt, err := sendReceiverInput(ctx, target, region, bridge.SignalIdentity{}, func(context.Context) (string, error) { return command, nil }, guard, progress)
	if err != nil {
		return receipt, attempt.ID, &BootstrapPendingError{ID: attempt.ID, Cause: err}
	}
	return receipt, attempt.ID, nil
}

func (a BootstrapReceiverAttempt) String() string {
	return fmt.Sprintf("%s %s %s", a.ID, a.Action, a.Phase)
}
