// Historical records only: no game capture, input, or execution adapters.
package live

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/vault"
	"path/filepath"
	"time"
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

type bootstrapCurrent struct {
	ID string `json:"id"`
}

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

func bootstrapCurrentKey(target ClientWindow, action string) string {
	digest := sha256.Sum256([]byte(windowResource(target) + "\x00" + action))
	return "bootstrap/current/" + hex.EncodeToString(digest[:])
}

func bootstrapWindowKey(target ClientWindow) string {
	digest := sha256.Sum256([]byte(windowResource(target)))
	return "bootstrap/window/" + hex.EncodeToString(digest[:])
}

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
		var fields map[string]json.RawMessage
		if err := json.Unmarshal(doc.Value, &fields); err != nil {
			return attempt, err
		}
		fields["phase"], _ = json.Marshal(attempt.Phase)
		fields["updatedAt"], _ = json.Marshal(attempt.UpdatedAt)
		raw, _ := json.Marshal(fields)
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

type receiverAttempt struct {
	Schema      string    `json:"schema"`
	OperationID string    `json:"operationId"`
	Action      string    `json:"action"`
	RequestID   string    `json:"requestId"`
	Arg         string    `json:"arg"`
	Attempt     int       `json:"attempt"`
	AttemptID   string    `json:"attemptId"`
	Nonce       string    `json:"receiverNonce,omitempty"`
	Epoch       uint64    `json:"runtimeEpoch,omitempty"`
	ReportScope string    `json:"reportScope,omitempty"`
	Phase       string    `json:"phase"`
	ReceiptKind string    `json:"receiptKind,omitempty"`
	Sequence    uint64    `json:"sequence,omitempty"`
	ErrorCode   string    `json:"errorCode,omitempty"`
	UpdatedAt   time.Time `json:"updatedAt"`
}
