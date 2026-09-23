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

type WorkIntent struct {
	Kind     string          `json:"kind"`
	Resource string          `json:"resource"`
	Snapshot string          `json:"snapshot"`
	Session  string          `json:"session"`
	Request  json.RawMessage `json:"request"`
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

// BeginWork reserves a resource durably. OS-lock release alone cannot make an
// unresolved operation disappear; a new operation must recover that record.
func (b *Book) BeginWork(ctx context.Context, intent WorkIntent) (WorkRecord, error) {
	return b.beginWork(ctx, intent, nil)
}

func (b *Book) beginWork(ctx context.Context, intent WorkIntent, reserve func(WorkRecord) error) (WorkRecord, error) {
	if intent.Kind != "probe" && intent.Kind != "faults" {
		return WorkRecord{}, errors.New("journal: unsupported work kind")
	}
	if intent.Resource == "" || len(intent.Resource) > 256 || !json.Valid(intent.Request) {
		return WorkRecord{}, errors.New("journal: invalid work intent")
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
	err = b.metadata.CommitDocuments(ctx, vault.Mutation{Key: "work/" + record.OperationID, Value: payload}, vault.Mutation{Key: key, ExpectedGeneration: owner.Generation, Value: claim})
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

func (b *Book) AdvanceStage(ctx context.Context, change StageChange) error {
	record, err := b.InspectWork(ctx, change.OperationID)
	if err != nil {
		return err
	}
	if record.Generation != change.ExpectedGeneration || record.Stage != change.ExpectedStage {
		return vault.ErrGeneration
	}
	if record.Status == "completed" || record.Status == "cancelled" {
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
	if change.Stage == "cleaned" {
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
	if to == "cleaned" {
		return from == "acknowledged" && (status == "completed" || status == "cancelled") ||
			from == "prepared" && status == "cancelled"
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
	stages := []string{"prepared", "load_requested", "loaded", "dispatch_requested", "reported", "flush_requested", "persisted", "verified", "ack_requested", "acknowledged", "cleaned"}
	for i := 0; i < len(stages)-1; i++ {
		if stages[i] == from {
			return stages[i+1] == to
		}
	}
	return false
}
