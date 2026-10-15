package records

import (
	"context"
	"errors"
	"runtime"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/records/resource"
	"github.com/follenfang/lycheedev/internal/vault"
)

func TestQueryAdmissionSharesBudgetAndPreservesLeaseOwner(t *testing.T) {
	ctx := context.Background()
	store, err := vault.Initialize(ctx, t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	cfg := vault.DefaultConfig()
	cfg.Download.Workers = 1
	if _, err := vault.WriteConfig(ctx, store.Root(), cfg); err != nil {
		t.Fatal(err)
	}
	query, admission, err := acquireQueryResources(ctx, store, FileQuery{})
	if err != nil {
		t.Fatal(err)
	}
	defer admission.Close()
	if runtime.GOOS == "windows" {
		if got := query.budget.MemorySnapshot(); got.Mode != "windows_adaptive" || got.ConcurrentQueries != 1 {
			t.Fatal("budget was created before configured admission", got)
		}
	}
	reader, err := OpenReader(store).NewSession(ctx, validReaderPin(), query)
	if err != nil {
		t.Fatal(err)
	}
	if reader.Query().budget != query.budget || reader.session.admission != nil {
		t.Fatal("nested reader replaced the budget or reacquired admission")
	}
	if err := reader.Close(); err != nil {
		t.Fatal(err)
	}
	blocked, cancel := context.WithTimeout(ctx, 40*time.Millisecond)
	defer cancel()
	if lease, err := store.AcquireDataResources(blocked, vault.DataOrdinary); lease != nil || !errors.Is(err, context.DeadlineExceeded) {
		if lease != nil {
			lease.Close()
		}
		t.Fatalf("nested close released its outer owner's lease: %v", err)
	}
	if err := admission.Close(); err != nil {
		t.Fatal(err)
	}
	// Embedders/tests that provide explicit limits retain those hard limits.
	explicit := resource.New(resource.Limits{RetainedBytes: 4, MetadataBytes: 4})
	owned, err := OpenReader(store).NewSession(ctx, validReaderPin(), FileQuery{budget: explicit})
	if err != nil {
		t.Fatal(err)
	}
	if owned.Query().budget != explicit || explicit.MemorySnapshot().Mode != "fixed" {
		t.Fatal("explicit fixed budget was silently enlarged")
	}
	if err := owned.Close(); err != nil {
		t.Fatal(err)
	}
	lease, err := store.AcquireDataResources(ctx, vault.DataMeasurement)
	if err != nil {
		t.Fatal("session-owned admission was not released", err)
	}
	lease.Close()
}
