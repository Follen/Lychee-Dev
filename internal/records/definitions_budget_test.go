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

func TestColdDefinitionsChargeActualMetadata(t *testing.T) {
	f := newDefinitionsFixture(t)
	limits := resource.DefaultLimits()
	// Enough for the actual definitions, but not two worst-case HTTP buffers.
	limits.MetadataBytes = 1 << 20
	cold := resource.New(limits)
	f.defs.budget = cold
	if _, err := f.defs.Prepare(context.Background(), definitionsTestCommit, "TestTable", false); err != nil {
		t.Fatal(err)
	}
	want := int64(len(definitionsManifest)+len(definitionsDBD)) * 4
	if got := cold.Snapshot(); got.MetadataBytes != want || got.RetainedBytes != want || cold.PeakScratchBytes() != 8<<20 || cold.RemainingRetained() != limits.RetainedBytes-want {
		t.Fatalf("cold accounting: %+v peak=%d", got, cold.PeakScratchBytes())
	}
	warm := resource.New(limits)
	f.defs.budget = warm
	if _, err := f.defs.Prepare(context.Background(), definitionsTestCommit, "TestTable", true); err != nil {
		t.Fatal(err)
	}
	if got := warm.Snapshot(); got.MetadataBytes != want || got.RetainedBytes != want || got.NetworkBytes != 0 {
		t.Fatalf("cached accounting: %+v", got)
	}
}

func TestColdDefinitionBudgetFailuresDoNotPublish(t *testing.T) {
	for _, dimension := range []string{"scratch", "metadata", "network"} {
		t.Run(dimension, func(t *testing.T) {
			f := newDefinitionsFixture(t)
			limits := resource.DefaultLimits()
			switch dimension {
			case "scratch":
				limits.RetainedBytes = (8 << 20) - 1
			case "metadata":
				limits.MetadataBytes = 1
			case "network":
				limits.NetworkBytes = 1
			}
			budget := resource.New(limits)
			f.defs.budget = budget
			if _, err := f.defs.Prepare(context.Background(), definitionsTestCommit, "TestTable", false); !errors.Is(err, resource.ErrBudget) {
				t.Fatalf("expected bounded failure: %v", err)
			}
			if budget.RemainingRetained() != limits.RetainedBytes-budget.Snapshot().RetainedBytes {
				t.Fatal("scratch leaked on failure")
			}
			f.defs.budget = resource.New(resource.DefaultLimits())
			if _, err := f.defs.Prepare(context.Background(), definitionsTestCommit, "TestTable", true); !errors.Is(err, ErrDefinitionUnavailable) {
				t.Fatalf("failed fetch published metadata: %v", err)
			}
		})
	}
}
