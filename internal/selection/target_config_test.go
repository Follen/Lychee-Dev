package selection

import (
	"context"
	"encoding/json"
	"errors"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/vault"
)

// fakePreparer resolves named targets through the NEW module interface only.
// Each call publishes a real immutable PinnedSet, so resolution behavior is
// tested end to end without internal/command or internal/records imports.
type fakePreparer struct {
	mu          sync.Mutex
	calls       int
	build       int
	before      func(call int) error
	unavailable bool
}

func (p *fakePreparer) PrepareTarget(ctx context.Context, workspace string, config TargetConfig, options PrepareOptions) (PinnedSet, error) {
	p.mu.Lock()
	p.calls++
	call := p.calls
	p.build++
	build := p.build
	if p.before != nil {
		hook := p.before
		p.mu.Unlock()
		if err := hook(call); err != nil {
			return PinnedSet{}, err
		}
	} else {
		p.mu.Unlock()
	}
	if p.unavailable {
		return PinnedSet{}, ErrTargetBuildUnavailable
	}
	// A moving "latest" legitimately produces a new identity per resolution.
	fullBuild := "12.1.0." + itoa(build)
	if config.FullBuild != "" {
		fullBuild = config.FullBuild
	}
	return vault.WriteMetadata(ctx, workspace, func(_ *vault.Store, metadata *vault.Metadata) (PinnedSet, error) {
		return OpenPinner(metadata).PinSelection(ctx, SelectionSpec{Data: &DataPin{
			Product:          config.Product,
			Region:           config.Region,
			Language:         config.Locale,
			FullBuild:        fullBuild,
			BuildConfig:      strings.Repeat("a", 32),
			CDNConfig:        strings.Repeat("b", 32),
			DefinitionCommit: strings.Repeat("c", 40),
		}})
	})
}

func itoa(value int) string {
	if value == 0 {
		return "0"
	}
	digits := ""
	for value > 0 {
		digits = string(rune('0'+value%10)) + digits
		value /= 10
	}
	return digits
}

func newTestWorkspace(t *testing.T) string {
	t.Helper()
	root := t.TempDir()
	if _, err := vault.Initialize(context.Background(), root); err != nil {
		t.Fatal(err)
	}
	return root
}

func testTarget(name string) TargetConfig {
	return TargetConfig{
		Name: name, Product: "retail", Region: "us", Locale: "enUS",
		Source: TargetSourceRemote, Repository: "wow-ui-source",
	}
}

func TestNamedTargetCreateListShowRemoveAndReplace(t *testing.T) {
	ctx := context.Background()
	root := newTestWorkspace(t)

	config := testTarget("retail-cn")
	config.Region = "cn"
	config.Locale = "zhCN"
	created, err := PutTarget(ctx, root, config, StoreTargetOptions{})
	if err != nil {
		t.Fatalf("PutTarget() error = %v", err)
	}
	if created.Schema != TargetSchema {
		t.Fatalf("schema = %q, want %q", created.Schema, TargetSchema)
	}
	if _, err := PutTarget(ctx, root, config, StoreTargetOptions{}); !errors.Is(err, ErrTargetExists) {
		t.Fatalf("second PutTarget() error = %v, want %v", err, ErrTargetExists)
	}

	replaced := config
	replaced.Locale = "enUS"
	if _, err := PutTarget(ctx, root, replaced, StoreTargetOptions{Replace: true}); err != nil {
		t.Fatalf("replace PutTarget() error = %v", err)
	}
	view, err := ShowTarget(ctx, root, "retail-cn")
	if err != nil {
		t.Fatalf("ShowTarget() error = %v", err)
	}
	if view.Config.Locale != "enUS" {
		t.Fatalf("replacement not applied: %v", view.Config)
	}

	if _, err := PutTarget(ctx, root, testTarget("retail-eu"), StoreTargetOptions{}); err != nil {
		t.Fatal(err)
	}
	list, err := ListTargets(ctx, root)
	if err != nil || len(list) != 2 {
		t.Fatalf("ListTargets() = %v, %v", list, err)
	}
	if _, err := LookupTarget(ctx, root, "retail"); !errors.Is(err, ErrTargetAmbiguous) {
		t.Fatalf("ambiguous prefix error = %v, want %v", err, ErrTargetAmbiguous)
	}
	if got, err := LookupTarget(ctx, root, "retail-eu"); err != nil || got.Name != "retail-eu" {
		t.Fatalf("LookupTarget() = %v, %v", got, err)
	}
	if _, err := LookupTarget(ctx, root, "nope"); !errors.Is(err, ErrTargetMissing) {
		t.Fatalf("missing error = %v, want %v", err, ErrTargetMissing)
	}

	report, err := RemoveTarget(ctx, root, "retail-cn")
	if err != nil {
		t.Fatalf("RemoveTarget() error = %v", err)
	}
	if report.Name != "retail-cn" {
		t.Fatalf("RemoveTarget() report = %v", report)
	}
	if _, err := ShowTarget(ctx, root, "retail-cn"); !errors.Is(err, ErrTargetMissing) {
		t.Fatalf("ShowTarget() after remove error = %v, want %v", err, ErrTargetMissing)
	}
}

func TestNamedTargetRequiresExplicitFields(t *testing.T) {
	ctx := context.Background()
	root := newTestWorkspace(t)
	missing := TargetConfig{Name: "retail", Region: "us", Locale: "enUS", Source: TargetSourceRemote}
	if _, err := PutTarget(ctx, root, missing, StoreTargetOptions{}); err == nil {
		t.Fatal("PutTarget() without product unexpectedly succeeded")
	}
	badSource := testTarget("retail")
	badSource.Source = "somewhere"
	if _, err := PutTarget(ctx, root, badSource, StoreTargetOptions{}); !errors.Is(err, ErrTargetFormat) {
		t.Fatalf("bad source error = %v, want %v", err, ErrTargetFormat)
	}
	local := testTarget("retail")
	local.Source = TargetSourceInstallation
	if _, err := PutTarget(ctx, root, local, StoreTargetOptions{}); !errors.Is(err, ErrTargetFormat) {
		t.Fatalf("local without path error = %v, want %v", err, ErrTargetFormat)
	}
	local.Installation = `C:\Games\World of Warcraft`
	if _, err := PutTarget(ctx, root, local, StoreTargetOptions{}); err != nil {
		t.Fatalf("local PutTarget() error = %v", err)
	}
}

func TestRemoveTargetProtectsRunningWorkAndResolvedPins(t *testing.T) {
	ctx := context.Background()
	root := newTestWorkspace(t)
	preparer := &fakePreparer{}
	if _, err := PutTarget(ctx, root, testTarget("retail"), StoreTargetOptions{}); err != nil {
		t.Fatal(err)
	}
	resolved, err := ResolveTarget(ctx, root, "retail", ResolveOptions{Preparer: preparer})
	if err != nil {
		t.Fatalf("ResolveTarget() error = %v", err)
	}
	view, err := ShowTarget(ctx, root, "retail")
	if err != nil || len(view.ResolvedPins) != 1 || view.ResolvedPins[0] != resolved.Pin.ID {
		t.Fatalf("ShowTarget() = %v, %v", view, err)
	}

	if err := RegisterTargetOperation(ctx, root, "retail", "OP-running"); err != nil {
		t.Fatal(err)
	}
	if _, err := RemoveTarget(ctx, root, "retail"); !errors.Is(err, ErrTargetInUse) {
		t.Fatalf("RemoveTarget() with running work error = %v, want %v", err, ErrTargetInUse)
	}
	if _, err := ShowTarget(ctx, root, "retail"); err != nil {
		t.Fatalf("target broken by refused removal: %v", err)
	}
	if err := ReleaseTargetOperation(ctx, root, "retail", "OP-running"); err != nil {
		t.Fatal(err)
	}
	report, err := RemoveTarget(ctx, root, "retail")
	if err != nil {
		t.Fatalf("RemoveTarget() error = %v", err)
	}
	if len(report.RemovedPins) != 1 || report.RemovedPins[0] != resolved.Pin.ID {
		t.Fatalf("RemoveTarget() report = %v", report)
	}
	// Removal must not break resolved pinned sets.
	pin, err := InspectSelection(ctx, root, resolved.Pin.ID)
	if err != nil || pin.ID != resolved.Pin.ID {
		t.Fatalf("resolved pin destroyed by target removal: %v, %v", pin, err)
	}
}

func TestResolveTargetProducesFreshIdentitiesAndRecords(t *testing.T) {
	ctx := context.Background()
	root := newTestWorkspace(t)
	preparer := &fakePreparer{}
	if _, err := PutTarget(ctx, root, testTarget("retail"), StoreTargetOptions{}); err != nil {
		t.Fatal(err)
	}
	first, err := ResolveTarget(ctx, root, "retail", ResolveOptions{Preparer: preparer})
	if err != nil {
		t.Fatal(err)
	}
	second, err := ResolveTarget(ctx, root, "retail", ResolveOptions{Preparer: preparer})
	if err != nil {
		t.Fatal(err)
	}
	// latest moved between the two command starts: each resolution keeps its
	// own identity and neither pins nor aliases the mutable configuration.
	if first.Pin.ID == second.Pin.ID {
		t.Fatalf("resolutions unexpectedly shared an identity: %s", first.Pin.ID)
	}
	view, err := ShowTarget(ctx, root, "retail")
	if err != nil || len(view.ResolvedPins) != 2 {
		t.Fatalf("ShowTarget() = %v, %v", view, err)
	}
}

func TestConcurrentResolutionsDoNotLeakPins(t *testing.T) {
	ctx := context.Background()
	root := newTestWorkspace(t)
	if _, err := PutTarget(ctx, root, testTarget("retail"), StoreTargetOptions{}); err != nil {
		t.Fatal(err)
	}
	const workers = 2
	preparer := &fakePreparer{}
	barrier := make(chan struct{})
	start := make(chan struct{})
	var wg sync.WaitGroup
	results := make([]ResolvedTarget, workers)
	errs := make([]error, workers)
	preparer.before = func(call int) error {
		<-start
		return nil
	}
	for i := 0; i < workers; i++ {
		wg.Add(1)
		go func(index int) {
			defer wg.Done()
			<-barrier
			results[index], errs[index] = ResolveTarget(ctx, root, "retail", ResolveOptions{Preparer: preparer})
		}(i)
	}
	close(barrier)
	time.Sleep(20 * time.Millisecond)
	close(start)
	wg.Wait()
	seen := map[string]bool{}
	for i := 0; i < workers; i++ {
		if errs[i] != nil {
			t.Fatalf("worker %d error = %v", i, errs[i])
		}
		pin := results[i].Pin
		if seen[pin.ID] {
			t.Fatalf("workers leaked a shared pin %s", pin.ID)
		}
		seen[pin.ID] = true
		if pin.Data == nil || pin.Data.Product != "retail" {
			t.Fatalf("worker %d pin = %v", i, pin)
		}
		if _, err := InspectSelection(ctx, root, pin.ID); err != nil {
			t.Fatalf("worker %d pin unreadable: %v", i, err)
		}
	}
	view, err := ShowTarget(ctx, root, "retail")
	if err != nil || len(view.ResolvedPins) != workers {
		t.Fatalf("ShowTarget() = %v, %v", view, err)
	}
}

func TestHistoricalBuildUnavailableIsPrecise(t *testing.T) {
	ctx := context.Background()
	root := newTestWorkspace(t)
	config := testTarget("retail")
	config.FullBuild = "12.1.0.60000"
	if _, err := PutTarget(ctx, root, config, StoreTargetOptions{}); err != nil {
		t.Fatal(err)
	}
	_, err := ResolveTarget(ctx, root, "retail", ResolveOptions{Preparer: &fakePreparer{unavailable: true}})
	if !errors.Is(err, ErrTargetBuildUnavailable) {
		t.Fatalf("ResolveTarget() error = %v, want %v", err, ErrTargetBuildUnavailable)
	}
}

func TestResolveIntentOrderingBeatsProjectLockWithConflictNotes(t *testing.T) {
	ctx := context.Background()
	root := newTestWorkspace(t)
	project := t.TempDir()

	locked := pinFor(t, root, "retail", "12.1.0.60001")
	if _, err := InitializeProject(ctx, project, "retail"); err != nil {
		t.Fatal(err)
	}
	if _, err := LockProject(ctx, root, project, locked.ID); err != nil {
		t.Fatal(err)
	}
	explicit := pinFor(t, root, "classic", "5.5.4.50001")

	// Explicit snapshot beats the project lock, and the conflict is reported
	// instead of silently overwriting either side.
	choice, err := ResolveIntent(ctx, root, IntentRequest{Snapshot: explicit.ID, Target: "", Project: project})
	if err != nil {
		t.Fatal(err)
	}
	if choice.Origin != "snapshot" || choice.Pin.ID != explicit.ID {
		t.Fatalf("ResolveIntent() = %v", choice)
	}
	if len(choice.Conflicts) == 0 || !strings.Contains(strings.Join(choice.Conflicts, "; "), "project lock") {
		t.Fatalf("missing conflict note: %v", choice.Conflicts)
	}

	// Explicit named target beats the project lock too.
	preparer := &fakePreparer{}
	if _, err := PutTarget(ctx, root, testTarget("retail"), StoreTargetOptions{}); err != nil {
		t.Fatal(err)
	}
	choice, err = ResolveIntent(ctx, root, IntentRequest{Target: "retail", Project: project, Preparer: preparer})
	if err != nil {
		t.Fatal(err)
	}
	if choice.Origin != "target" || choice.TargetName != "retail" {
		t.Fatalf("ResolveIntent() = %v", choice)
	}

	// The project lock is the last fallback and never invented.
	choice, err = ResolveIntent(ctx, root, IntentRequest{Project: project})
	if err != nil {
		t.Fatal(err)
	}
	if choice.Origin != "project-lock" || choice.Pin.ID != locked.ID {
		t.Fatalf("ResolveIntent() = %v", choice)
	}

	if _, err := ResolveIntent(ctx, root, IntentRequest{}); !errors.Is(err, ErrTargetRequired) {
		t.Fatalf("empty intent error = %v, want %v", err, ErrTargetRequired)
	}
}

// CON-11 (metadata competition): concurrent writes to one mutable target have
// stable, precise outcomes and never half-commit.
func TestConcurrentTargetWritesReportStableErrors(t *testing.T) {
	ctx := context.Background()
	root := newTestWorkspace(t)
	const workers = 2
	var wg sync.WaitGroup
	start := make(chan struct{})
	errs := make([]error, workers)
	for i := 0; i < workers; i++ {
		wg.Add(1)
		go func(index int) {
			defer wg.Done()
			<-start
			config := testTarget("retail")
			config.FullBuild = "12.1.0.6000" + itoa(index)
			_, errs[index] = PutTarget(ctx, root, config, StoreTargetOptions{})
		}(i)
	}
	close(start)
	wg.Wait()
	succeeded := 0
	for _, err := range errs {
		switch {
		case err == nil:
			succeeded++
		case errors.Is(err, ErrTargetExists), errors.Is(err, vault.ErrGeneration):
			// Stable, documented conflict outcomes.
		default:
			t.Fatalf("unstable error = %v", err)
		}
	}
	if succeeded != 1 {
		t.Fatalf("succeeded = %d, want exactly 1 winner", succeeded)
	}
	view, err := ShowTarget(ctx, root, "retail")
	if err != nil || view.Config.FullBuild == "" {
		t.Fatalf("target corrupted by concurrent create: %+v, %v", view, err)
	}
}

func pinFor(t *testing.T, root, product, fullBuild string) PinnedSet {
	t.Helper()
	pin, err := vault.WriteMetadata(context.Background(), root, func(_ *vault.Store, metadata *vault.Metadata) (PinnedSet, error) {
		return OpenPinner(metadata).PinSelection(context.Background(), SelectionSpec{Data: &DataPin{
			Product:          product,
			Region:           "us",
			Language:         "enUS",
			FullBuild:        fullBuild,
			BuildConfig:      strings.Repeat("a", 32),
			CDNConfig:        strings.Repeat("b", 32),
			DefinitionCommit: strings.Repeat("c", 40),
		}})
	})
	if err != nil {
		t.Fatal(err)
	}
	raw, err := json.Marshal(pin)
	if err != nil || len(raw) == 0 {
		t.Fatal(err)
	}
	return pin
}
