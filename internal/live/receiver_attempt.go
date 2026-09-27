package live

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/vault"
)

// receiverAttempt is intentionally separate from the business work stage. It
// records wake and every possible commit boundary before the native key is
// queued. A missing accepted receipt cannot turn into permission to replay.
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

func (p *ProbeOperation) recordReceiverProgress(ctx context.Context, phase string, stage bridge.ReceiverStage, attempt int, receipt bridge.Signal) error {
	return saveReceiverAttempt(ctx, p.metadata, p.id, phase, stage, attempt, receipt)
}

func (p *faultOperation) recordReceiverProgress(ctx context.Context, phase string, stage bridge.ReceiverStage, attempt int, receipt bridge.Signal) error {
	return saveReceiverAttempt(ctx, p.metadata, p.id, phase, stage, attempt, receipt)
}

func saveReceiverAttempt(ctx context.Context, metadata *vault.Metadata, id, phase string, stage bridge.ReceiverStage, attempt int, receipt bridge.Signal) error {
	if metadata == nil || id == "" {
		return errors.New("live.receiver_attempt_store_missing")
	}
	persist, cancel := context.WithTimeout(context.WithoutCancel(ctx), 5*time.Second)
	defer cancel()
	key := receiverAttemptKey(id, stage.Action, stage.RequestID, stage.Arg)
	doc, err := metadata.ReadDocument(persist, key)
	if err != nil && !errors.Is(err, vault.ErrMissingRecord) {
		return err
	}
	var prior receiverAttempt
	if err == nil {
		if json.Unmarshal(doc.Value, &prior) != nil || prior.Schema != "lycheedev.receiver-attempt.v1" || prior.OperationID != id || prior.Action != stage.Action || prior.RequestID != stage.RequestID || prior.Arg != stage.Arg {
			return errors.New("live.receiver_attempt_conflict")
		}
		if phase == "wake_requested" {
			if (prior.Phase != "rejected" && prior.Phase != "unsent") || prior.Attempt >= 3 {
				return fmt.Errorf("live.receiver_attempt_unresolved: %s", prior.Phase)
			}
		}
		if phase != "wake_requested" && (prior.AttemptID != stage.AttemptID || prior.Phase == "dismissed") {
			return errors.New("live.receiver_attempt_conflict")
		}
	}
	number := 1
	if err == nil {
		number = prior.Attempt
		if phase == "wake_requested" {
			number++
		}
	}
	if number > 3 || attempt < 0 || attempt > 2 {
		return errors.New("live.receiver_retry_limit")
	}
	next := receiverAttempt{Schema: "lycheedev.receiver-attempt.v1", OperationID: id,
		Action: stage.Action, RequestID: stage.RequestID, Arg: stage.Arg,
		Attempt: number, AttemptID: stage.AttemptID, Nonce: stage.ReceiverNonce,
		Epoch: stage.RuntimeEpoch, ReportScope: receipt.ReportScope, Phase: phase, ReceiptKind: receipt.Kind,
		Sequence: receipt.Sequence, ErrorCode: receipt.ErrorCode, UpdatedAt: time.Now().UTC()}
	if next.ReportScope == "" && phase != "wake_requested" {
		next.ReportScope = prior.ReportScope
	}
	if phase != "wake_requested" && prior.Phase == "" {
		return errors.New("live.receiver_attempt_missing")
	}
	raw, _ := json.Marshal(next)
	return metadata.CommitDocuments(persist, vault.Mutation{Key: key, ExpectedGeneration: doc.Generation, Value: raw})
}

func receiverAttemptKey(id, action, request, arg string) string {
	digest := sha256.Sum256([]byte(action + "\x00" + request + "\x00" + arg))
	return "receiver/" + id + "/" + hex.EncodeToString(digest[:])
}

func receiverAttemptForWork(ctx context.Context, metadata *vault.Metadata, id, action, request, arg string) (receiverAttempt, error) {
	doc, err := metadata.ReadDocument(ctx, receiverAttemptKey(id, action, request, arg))
	if err != nil {
		return receiverAttempt{}, err
	}
	var attempt receiverAttempt
	if err := json.Unmarshal(doc.Value, &attempt); err != nil {
		return attempt, err
	}
	if attempt.Schema != "lycheedev.receiver-attempt.v1" || attempt.OperationID != id || attempt.Action != action || attempt.RequestID != request || attempt.Arg != arg {
		return attempt, errors.New("live.receiver_attempt_corrupt")
	}
	return attempt, nil
}

func receiverAttemptBlocksReplay(attempt receiverAttempt) bool {
	return attempt.Phase != "rejected"
}
