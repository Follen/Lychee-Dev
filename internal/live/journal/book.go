// Package journal owns durable game-operation intent and resource admission.
package journal

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"time"

	"github.com/follenfang/lycheedev/internal/vault"
)

var ErrBusy = errors.New("journal.resource_busy")
var ErrTransition = errors.New("journal.invalid_transition")
var ErrRequestConflict = errors.New("journal.request_conflict")

type WorkIntent struct {
	Kind          string          `json:"kind"`
	Resource      string          `json:"resource"`
	Snapshot      string          `json:"snapshot"`
	Session       string          `json:"session"`
	Request       json.RawMessage `json:"request"`
	RequestKey    string          `json:"requestKey,omitempty"`
	RequestDigest string          `json:"requestDigest,omitempty"`
	Goal          string          `json:"goal,omitempty"`
}

type WorkRecord struct {
	Schema      string          `json:"schema"`
	OperationID string          `json:"operationId"`
	Generation  int64           `json:"generation"`
	Intent      WorkIntent      `json:"intent"`
	Stage       string          `json:"stage"`
	Status      string          `json:"status"`
	CreatedAt   time.Time       `json:"createdAt"`
	UpdatedAt   time.Time       `json:"updatedAt"`
	Observation json.RawMessage `json:"observation"`
}

type StageChange struct {
	OperationID        string
	ExpectedGeneration int64
	ExpectedStage      string
	Stage              string
	Status             string
	Observation        json.RawMessage
}

type Book struct{ metadata *vault.Metadata }

func OpenBook(metadata *vault.Metadata) *Book { return &Book{metadata: metadata} }

type ownership struct {
	OperationID string `json:"operationId"`
}

type requestIdentity struct {
	OperationID string `json:"operationId"`
	Digest      string `json:"digest"`
}

func (b *Book) resolveRequest(ctx context.Context, intent WorkIntent) (WorkRecord, bool, error) {
	if intent.RequestKey == "" {
		return WorkRecord{}, false, nil
	}
	if len(intent.RequestKey) > 128 || len(intent.RequestDigest) != 64 {
		return WorkRecord{}, false, errors.New("journal.invalid_request_identity")
	}
	if _, err := hex.DecodeString(intent.RequestDigest); err != nil {
		return WorkRecord{}, false, errors.New("journal.invalid_request_identity")
	}
	doc, err := b.metadata.ReadDocument(ctx, "request/"+intent.RequestKey)
	if errors.Is(err, vault.ErrMissingRecord) {
		return WorkRecord{}, false, nil
	}
	if err != nil {
		return WorkRecord{}, false, err
	}
	var identity requestIdentity
	if err := json.Unmarshal(doc.Value, &identity); err != nil || identity.OperationID == "" || len(identity.Digest) != 64 {
		return WorkRecord{}, false, errors.New("journal.corrupt_request_identity")
	}
	if identity.Digest != intent.RequestDigest {
		return WorkRecord{}, false, ErrRequestConflict
	}
	record, err := b.InspectWork(ctx, identity.OperationID)
	if err != nil {
		return WorkRecord{}, false, err
	}
	return record, true, nil
}

// BeginWork reserves a resource durably. OS-lock release alone cannot make an
// unresolved operation disappear; a new operation must recover that record.
func (b *Book) BeginWork(ctx context.Context, intent WorkIntent) (WorkRecord, error) {
	return b.beginWork(ctx, intent, nil)
}

func (b *Book) beginWork(ctx context.Context, intent WorkIntent, reserve func(WorkRecord) error) (WorkRecord, error) {
	if intent.Kind != "probe" && intent.Kind != "faults" && intent.Kind != "reload" {
		return WorkRecord{}, errors.New("journal: unsupported work kind")
	}
	if intent.Resource == "" || len(intent.Resource) > 256 || !json.Valid(intent.Request) {
		return WorkRecord{}, errors.New("journal: invalid work intent")
	}
	if existing, found, err := b.resolveRequest(ctx, intent); err != nil || found {
		return existing, err
	}
	key := "ownership/" + intent.Resource
	owner, err := b.metadata.ReadDocument(ctx, key)
	if err != nil && !errors.Is(err, vault.ErrMissingRecord) {
		return WorkRecord{}, err
	}
	if err == nil {
		var current ownership
		if err := json.Unmarshal(owner.Value, &current); err != nil {
			return WorkRecord{}, err
		}
		if current.OperationID != "" {
			return WorkRecord{}, fmt.Errorf("%w: %s", ErrBusy, current.OperationID)
		}
	}
	var nonce [16]byte
	if _, err := rand.Read(nonce[:]); err != nil {
		return WorkRecord{}, err
	}
	now := time.Now().UTC()
	record := WorkRecord{Schema: "lycheedev.work.v1", OperationID: "OP-" + hex.EncodeToString(nonce[:]), Generation: 1, Intent: intent, Stage: "prepared", Status: "pending", CreatedAt: now, UpdatedAt: now, Observation: json.RawMessage(`null`)}
	payload, err := json.Marshal(record)
	if err != nil {
		return WorkRecord{}, err
	}
	claim, _ := json.Marshal(ownership{OperationID: record.OperationID})
	if reserve != nil {
		if err := reserve(record); err != nil {
			return record, err
		}
	}
	mutations := []vault.Mutation{{Key: "work/" + record.OperationID, Value: payload}, {Key: key, ExpectedGeneration: owner.Generation, Value: claim}}
	if intent.RequestKey != "" {
		request, _ := json.Marshal(requestIdentity{OperationID: record.OperationID, Digest: intent.RequestDigest})
		mutations = append(mutations, vault.Mutation{Key: "request/" + intent.RequestKey, Value: request})
	}
	err = b.metadata.CommitDocuments(ctx, mutations...)
	if errors.Is(err, vault.ErrGeneration) {
		return WorkRecord{}, ErrBusy
	}
	return record, err
}

func (b *Book) InspectWork(ctx context.Context, id string) (WorkRecord, error) {
	doc, err := b.metadata.ReadDocument(ctx, "work/"+id)
	if err != nil {
		return WorkRecord{}, err
	}
	var record WorkRecord
	if err := json.Unmarshal(doc.Value, &record); err != nil {
		return record, err
	}
	if record.Schema != "lycheedev.work.v1" || record.OperationID != id || record.Generation != doc.Generation {
		return record, errors.New("journal: corrupt work record")
	}
	return record, nil
}

// SetGoal records the terminal promised by the next explicit public command.
// Goals only move forward; resume never invents or broadens one.
func (b *Book) SetGoal(ctx context.Context, id, expectedStage, goal string) (WorkRecord, error) {
	record, err := b.InspectWork(ctx, id)
	if err != nil {
		return record, err
	}
	if record.Stage == "abandoning" || record.Stage == "abandoned" {
		return record, ErrTransition
	}
	if record.Intent.Goal == goal {
		return record, nil
	}
	if record.Stage != expectedStage || record.Status != "running" && record.Status != "unresolved" {
		return record, ErrTransition
	}
	allowed := record.Intent.Goal == "loaded" && goal == "verified" || record.Intent.Goal == "verified" && goal == "cleaned"
	if !allowed {
		return record, ErrTransition
	}
	record.Intent.Goal = goal
	record.Generation++
	record.UpdatedAt = time.Now().UTC()
	payload, err := json.Marshal(record)
	if err != nil {
		return WorkRecord{}, err
	}
	if err := b.metadata.CommitDocuments(ctx, vault.Mutation{Key: "work/" + id, ExpectedGeneration: record.Generation - 1, Value: payload}); err != nil {
		return WorkRecord{}, err
	}
	return record, nil
}

func (b *Book) AdvanceStage(ctx context.Context, change StageChange) error {
	record, err := b.InspectWork(ctx, change.OperationID)
	if err != nil {
		return err
	}
	if record.Generation != change.ExpectedGeneration || record.Stage != change.ExpectedStage {
		return vault.ErrGeneration
	}
	if record.Status == "completed" || record.Status == "cancelled" || record.Status == "abandoned" {
		return ErrTransition
	}
	if !allowsTransition(record.Intent.Kind, record.Stage, change.Stage, change.Status) {
		return ErrTransition
	}
	if !json.Valid(change.Observation) {
		return errors.New("journal: invalid observation")
	}
	record.Stage = change.Stage
	record.Status = change.Status
	record.Observation = change.Observation
	record.Generation++
	record.UpdatedAt = time.Now().UTC()
	payload, err := json.Marshal(record)
	if err != nil {
		return err
	}
	mutations := []vault.Mutation{{Key: "work/" + record.OperationID, ExpectedGeneration: change.ExpectedGeneration, Value: payload}}
	if change.Stage == "cleaned" || change.Stage == "abandoned" {
		key := "ownership/" + record.Intent.Resource
		owner, err := b.metadata.ReadDocument(ctx, key)
		if err != nil {
			return err
		}
		var current ownership
		if err := json.Unmarshal(owner.Value, &current); err != nil {
			return err
		}
		if current.OperationID != record.OperationID {
			return ErrBusy
		}
		mutations = append(mutations, vault.Mutation{Key: key, ExpectedGeneration: owner.Generation, Value: json.RawMessage(`{"operationId":""}`)})
	}
	return b.metadata.CommitDocuments(ctx, mutations...)
}

func allowsTransition(kind, from, to, status string) bool {
	if from == "abandoning" || to == "abandoning" || to == "abandoned" {
		return kind == "probe" && (from == "verified" && to == "abandoning" && status == "running" || from == "dispatch_requested" && to == "abandoning" && status == "running" || from == "flush_requested" && to == "abandoning" && status == "running" || from == "abandoning" && to == "abandoned" && status == "abandoned")
	}
	if to == "cleaned" {
		return from == "acknowledged" && (status == "completed" || status == "cancelled") ||
			from == "prepared" && status == "cancelled" ||
			kind == "reload" && from == "reload_requested" && status == "completed"
	}
	if status != "running" && status != "unresolved" && status != "failed" {
		return false
	}
	if from == to {
		return true
	} // Observation or failure never implies a new effect.
	if kind == "faults" && from == "prepared" && to == "dispatch_requested" {
		return true
	}
	if kind == "reload" && from == "prepared" && to == "reload_requested" {
		return true
	}
	stages := []string{"prepared", "load_requested", "loaded", "dispatch_requested", "reported", "flush_requested", "persisted", "verified", "ack_requested", "acknowledged", "cleaned"}
	for i := 0; i < len(stages)-1; i++ {
		if stages[i] == from {
			return stages[i+1] == to
		}
	}
	return false
}
