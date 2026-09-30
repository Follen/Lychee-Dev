package records

import (
	"context"
	"errors"
	"testing"

	"github.com/follenfang/lycheedev/internal/records/resource"
)

func TestDefinitionsBudgetSharedNetworkAndOfflinePreparation(t *testing.T) {
	fixture := newDefinitionsFixture(t)
	limits := resource.DefaultLimits()
	limits.NetworkRequests = 1
	budget := resource.New(limits)
	fixture.defs.WithBudget(budget)
	_, err := fixture.defs.Prepare(context.Background(), definitionsTestCommit, "TestTable", false)
	if !errors.Is(err, resource.ErrBudget) || len(fixture.transport.callURLs()) != 1 {
		t.Fatalf("requests reset: %v %v", err, fixture.transport.callURLs())
	}
	if budget.Snapshot().NetworkBytes != int64(len(definitionsManifest)) {
		t.Fatal(budget.Snapshot())
	}
	limits = resource.DefaultLimits()
	limits.MetadataBytes = 1
	fixture.defs.WithBudget(resource.New(limits))
	_, err = fixture.defs.Prepare(context.Background(), definitionsTestCommit, "TestTable", true)
	if !errors.Is(err, resource.ErrBudget) {
		t.Fatalf("offline metadata uncharged: %v", err)
	}
}

func TestDefinitionsBudgetChecksUnknownLengthResponse(t *testing.T) {
	fixture := newDefinitionsFixture(t)
	limits := resource.DefaultLimits()
	limits.NetworkBytes = 3
	budget := resource.New(limits)
	fixture.defs.WithBudget(budget)
	_, err := fixture.defs.Prepare(context.Background(), definitionsTestCommit, "TestTable", false)
	if !errors.Is(err, resource.ErrBudget) {
		t.Fatalf("unbounded unknown-length body: %v", err)
	}
	if budget.Snapshot().NetworkBytes > 3 || len(fixture.transport.callURLs()) != 1 {
		t.Fatal(budget.Snapshot(), fixture.transport.callURLs())
	}
}
