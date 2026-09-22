package journal

import (
	"context"
	"encoding/json"
	"errors"
	"path/filepath"
	"testing"

	"github.com/follenfang/lycheedev/internal/vault"
)

func newTestBook(t *testing.T) (string, *vault.Metadata, *Book) {
	t.Helper()
	ctx := context.Background()
	root := filepath.Join(t.TempDir(), "workspace")
	store, err := vault.Initialize(ctx, root)
	if err != nil {
		t.Fatal(err)
	}
	metadata, err := store.OpenMetadata(ctx)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = metadata.Close() })
	return root, metadata, OpenBook(metadata)
}

func testIntent(resource string) WorkIntent {
	return WorkIntent{
		Kind:     "probe",
		Resource: resource,
		Snapshot: "snapshot-1",
		Session:  "session-1",
		Request:  json.RawMessage(`{"query":"unit-test"}`),
	}
}

func advanceBookStage(t *testing.T, book *Book, record WorkRecord, stage, status string) WorkRecord {
	t.Helper()
	ctx := context.Background()
	if err := book.AdvanceStage(ctx, StageChange{
		OperationID:        record.OperationID,
		ExpectedGeneration: record.Generation,
		ExpectedStage:      record.Stage,
		Stage:              stage,
		Status:             status,
		Observation:        json.RawMessage(`{"observed":true}`),
	}); err != nil {
		t.Fatalf("AdvanceStage(%s) error = %v", stage, err)
	}
	updated, err := book.InspectWork(ctx, record.OperationID)
	if err != nil {
		t.Fatalf("InspectWork(%s) error = %v", stage, err)
	}
	if updated.Generation != record.Generation+1 || updated.Stage != stage || updated.Status != status {
		t.Fatalf("stage %s produced %+v, want generation %d and status %s", stage, updated, record.Generation+1, status)
	}
	return updated
}

func advanceToAcknowledged(t *testing.T, book *Book, record WorkRecord) WorkRecord {
	t.Helper()
	for _, stage := range []string{
		"load_requested",
		"loaded",
		"dispatch_requested",
		"reported",
		"flush_requested",
		"persisted",
		"verified",
		"ack_requested",
		"acknowledged",
	} {
		record = advanceBookStage(t, book, record, stage, "running")
	}
	return record
}

func TestBookProbeStagesPersistAcrossReopen(t *testing.T) {
	ctx := context.Background()
	root, metadata, book := newTestBook(t)

	record, err := book.BeginWork(ctx, testIntent("resource/probe"))
	if err != nil {
		t.Fatalf("BeginWork() error = %v", err)
	}
	if record.Generation != 1 || record.Stage != "prepared" || record.Status != "pending" {
		t.Fatalf("initial record = %+v", record)
	}

	if err := metadata.Close(); err != nil {
		t.Fatal(err)
	}
	reopenedStore, err := vault.OpenStore(root)
	if err != nil {
		t.Fatal(err)
	}
	reopenedMetadata, err := reopenedStore.OpenMetadata(ctx)
	if err != nil {
		t.Fatal(err)
	}
	defer reopenedMetadata.Close()
	book = OpenBook(reopenedMetadata)

	persisted, err := book.InspectWork(ctx, record.OperationID)
	if err != nil {
		t.Fatalf("InspectWork after reopen error = %v", err)
	}
	if persisted.OperationID != record.OperationID || persisted.Generation != 1 || persisted.Stage != "prepared" {
		t.Fatalf("reopened record = %+v", persisted)
	}

	persisted = advanceToAcknowledged(t, book, persisted)
	persisted = advanceBookStage(t, book, persisted, "cleaned", "completed")
	if persisted.Stage != "cleaned" || persisted.Status != "completed" || persisted.Generation != 11 {
		t.Fatalf("cleaned record = %+v", persisted)
	}

	owner, err := reopenedMetadata.ReadDocument(ctx, "ownership/resource/probe")
	if err != nil {
		t.Fatalf("read released ownership: %v", err)
	}
	if owner.Generation != 2 || string(owner.Value) != `{"operationId":""}` {
		t.Fatalf("released ownership = generation %d, value %s", owner.Generation, owner.Value)
	}
}

func TestBookRejectsStageSkip(t *testing.T) {
	ctx := context.Background()
	_, _, book := newTestBook(t)

	record, err := book.BeginWork(ctx, testIntent("resource/skip"))
	if err != nil {
		t.Fatal(err)
	}
	if err := book.AdvanceStage(ctx, StageChange{
		OperationID:        record.OperationID,
		ExpectedGeneration: record.Generation,
		ExpectedStage:      "prepared",
		Stage:              "loaded",
		Status:             "running",
		Observation:        json.RawMessage(`{"observed":true}`),
	}); !errors.Is(err, ErrTransition) {
		t.Fatalf("stage skip error = %v, want %v", err, ErrTransition)
	}

	unchanged, err := book.InspectWork(ctx, record.OperationID)
	if err != nil {
		t.Fatal(err)
	}
	if unchanged.Generation != 1 || unchanged.Stage != "prepared" || unchanged.Status != "pending" {
		t.Fatalf("stage skip changed record = %+v", unchanged)
	}
}

func TestBookRejectsStaleGeneration(t *testing.T) {
	ctx := context.Background()
	_, _, book := newTestBook(t)

	record, err := book.BeginWork(ctx, testIntent("resource/stale"))
	if err != nil {
		t.Fatal(err)
	}
	current := advanceBookStage(t, book, record, "load_requested", "running")

	err = book.AdvanceStage(ctx, StageChange{
		OperationID:        record.OperationID,
		ExpectedGeneration: record.Generation,
		ExpectedStage:      record.Stage,
		Stage:              "loaded",
		Status:             "running",
		Observation:        json.RawMessage(`{"observed":true}`),
	})
	if !errors.Is(err, vault.ErrGeneration) {
		t.Fatalf("stale generation error = %v, want %v", err, vault.ErrGeneration)
	}

	unchanged, err := book.InspectWork(ctx, record.OperationID)
	if err != nil {
		t.Fatal(err)
	}
	if unchanged.Generation != current.Generation || unchanged.Stage != current.Stage {
		t.Fatalf("stale generation changed record = %+v, want %+v", unchanged, current)
	}
}

func TestBookUnresolvedRetainsOwnershipAndIndependentResourceStarts(t *testing.T) {
	ctx := context.Background()
	_, metadata, book := newTestBook(t)

	record, err := book.BeginWork(ctx, testIntent("resource/unresolved"))
	if err != nil {
		t.Fatal(err)
	}
	unresolved := advanceBookStage(t, book, record, "load_requested", "unresolved")

	owner, err := metadata.ReadDocument(ctx, "ownership/resource/unresolved")
	if err != nil {
		t.Fatal(err)
	}
	if owner.Generation != 1 || string(owner.Value) != `{"operationId":"`+record.OperationID+`"}` {
		t.Fatalf("unresolved ownership = generation %d, value %s", owner.Generation, owner.Value)
	}

	if _, err := book.BeginWork(ctx, testIntent("resource/unresolved")); !errors.Is(err, ErrBusy) {
		t.Fatalf("reusing unresolved resource error = %v, want %v", err, ErrBusy)
	}

	independent, err := book.BeginWork(ctx, testIntent("resource/independent"))
	if err != nil {
		t.Fatalf("independent resource BeginWork() error = %v", err)
	}
	if independent.OperationID == unresolved.OperationID {
		t.Fatal("independent resource reused unresolved operation ID")
	}
}

func TestBookCleanedReleasesOwnershipAtomically(t *testing.T) {
	ctx := context.Background()
	_, metadata, book := newTestBook(t)

	record, err := book.BeginWork(ctx, testIntent("resource/clean"))
	if err != nil {
		t.Fatal(err)
	}
	acknowledged := advanceToAcknowledged(t, book, record)

	owner, err := metadata.ReadDocument(ctx, "ownership/resource/clean")
	if err != nil {
		t.Fatal(err)
	}
	if err := metadata.CommitDocuments(ctx, vault.Mutation{
		Key:                owner.Key,
		ExpectedGeneration: owner.Generation,
		Value:              json.RawMessage(`{"operationId":"other-operation"}`),
	}); err != nil {
		t.Fatal(err)
	}
	if err := book.AdvanceStage(ctx, StageChange{
		OperationID:        acknowledged.OperationID,
		ExpectedGeneration: acknowledged.Generation,
		ExpectedStage:      acknowledged.Stage,
		Stage:              "cleaned",
		Status:             "completed",
		Observation:        json.RawMessage(`{"observed":true}`),
	}); !errors.Is(err, ErrBusy) {
		t.Fatalf("cleanup with foreign owner error = %v, want %v", err, ErrBusy)
	}

	unchanged, err := book.InspectWork(ctx, acknowledged.OperationID)
	if err != nil {
		t.Fatal(err)
	}
	if unchanged.Generation != acknowledged.Generation || unchanged.Stage != "acknowledged" {
		t.Fatalf("failed cleanup partially changed work = %+v", unchanged)
	}
	foreign, err := metadata.ReadDocument(ctx, owner.Key)
	if err != nil {
		t.Fatal(err)
	}
	if string(foreign.Value) != `{"operationId":"other-operation"}` {
		t.Fatalf("failed cleanup changed ownership = %s", foreign.Value)
	}

	if err := metadata.CommitDocuments(ctx, vault.Mutation{
		Key:                owner.Key,
		ExpectedGeneration: foreign.Generation,
		Value:              json.RawMessage(`{"operationId":"` + acknowledged.OperationID + `"}`),
	}); err != nil {
		t.Fatal(err)
	}
	cleaned := advanceBookStage(t, book, acknowledged, "cleaned", "completed")
	if cleaned.Stage != "cleaned" || cleaned.Status != "completed" {
		t.Fatalf("cleaned record = %+v", cleaned)
	}

	released, err := metadata.ReadDocument(ctx, owner.Key)
	if err != nil {
		t.Fatal(err)
	}
	if released.Generation != foreign.Generation+2 || string(released.Value) != `{"operationId":""}` {
		t.Fatalf("released ownership = generation %d, value %s", released.Generation, released.Value)
	}
	if _, err := book.BeginWork(ctx, testIntent("resource/clean")); err != nil {
		t.Fatalf("new work after cleanup error = %v", err)
	}
}
