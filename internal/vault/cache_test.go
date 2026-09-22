package vault

import (
	"context"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"
)

func newCacheWorkspace(t *testing.T) *Store {
	t.Helper()
	store, err := Initialize(context.Background(), t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	return store
}

func putObject(t *testing.T, cache *CacheManager, content string, retain *CacheRetainOwner) CacheRef {
	t.Helper()
	ref, err := cache.Put(context.Background(), CacheInput{Reader: strings.NewReader(content), MaxBytes: 1 << 20, Retain: retain})
	if err != nil {
		t.Fatalf("Put(%q) error = %v", content, err)
	}
	return ref
}

func TestCacheStatusAccountingAndVisibility(t *testing.T) {
	ctx := context.Background()
	store := newCacheWorkspace(t)
	cache := store.Cache()
	ordinary := putObject(t, cache, "ordinary-object", nil)
	pinned := putObject(t, cache, "pinned-object", &CacheRetainOwner{Kind: RetainPinned, Owner: "PIN-1"})
	if err := cache.Retain(ctx, pinned, CacheRetainOwner{Kind: RetainEvidence, Owner: "CAP-1"}); err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(cache.stagingDir(), 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(cache.stagingDir(), "put-abcdef.part"), []byte("12345"), 0600); err != nil {
		t.Fatal(err)
	}

	status, err := cache.Status(ctx)
	if err != nil {
		t.Fatalf("Status() error = %v", err)
	}
	if status.ObjectCount != 2 {
		t.Fatalf("objectCount = %d", status.ObjectCount)
	}
	wantBytes := ordinary.Bytes + pinned.Bytes
	if status.ObjectBytes != wantBytes {
		t.Fatalf("objectBytes = %d, want %d", status.ObjectBytes, wantBytes)
	}
	if status.UncommittedCount != 1 || status.UncommittedBytes != 5 {
		t.Fatalf("uncommitted = %d / %d", status.UncommittedCount, status.UncommittedBytes)
	}
	if status.ProtectedCount != 1 || status.ProtectedByKind[RetainPinned] != 1 || status.ProtectedByKind[RetainEvidence] != 1 {
		t.Fatalf("protection = %+v", status)
	}
	if len(status.Protected) != 1 || len(status.Protected[0].Owners) != 2 {
		t.Fatalf("visibility = %+v", status.Protected)
	}
	if status.LimitBytes != DefaultCacheMaxBytes || !status.WithinLimit {
		t.Fatalf("limit = %+v", status)
	}
}

func TestCacheVerifyDetectsCorruptionAndMissing(t *testing.T) {
	ctx := context.Background()
	store := newCacheWorkspace(t)
	cache := store.Cache()
	good := putObject(t, cache, "good-content", nil)
	bad := putObject(t, cache, "bad-content", nil)

	report, err := cache.Verify(ctx)
	if err != nil || !report.OK || report.Checked != 2 {
		t.Fatalf("Verify() = %+v, %v", report, err)
	}

	// Corrupt one cached object in place.
	path := cache.objectPath(bad.SHA256)
	raw, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	raw[0] ^= 0xff
	if err := os.WriteFile(path, raw, 0600); err != nil {
		t.Fatal(err)
	}
	// A retain marker without its object is reported as missing, never fixed.
	if err := cache.Retain(ctx, CacheRef{SHA256: strings.Repeat("e", 64), Bytes: 3}, CacheRetainOwner{Kind: RetainOperation, Owner: "OP-1"}); err != nil {
		t.Fatal(err)
	}

	report, err = cache.Verify(ctx)
	if err != nil {
		t.Fatal(err)
	}
	if report.OK || len(report.Corrupt) != 1 || report.Corrupt[0] != bad.SHA256 {
		t.Fatalf("Verify() = %+v", report)
	}
	if len(report.Missing) != 1 || report.Missing[0] != strings.Repeat("e", 64) {
		t.Fatalf("Verify() missing = %+v", report)
	}
	// Read refuses corrupted bytes instead of returning them.
	if _, err := cache.Read(ctx, bad, 1<<20); !errors.Is(err, ErrCacheIntegrity) {
		t.Fatalf("Read(corrupt) error = %v, want %v", err, ErrCacheIntegrity)
	}
	if data, err := cache.Read(ctx, good, 1<<20); err != nil || string(data) != "good-content" {
		t.Fatalf("Read(good) = %q, %v", data, err)
	}
}

// STO-09 and CON-07: pruning reclaims ordinary cache only. Pinned objects,
// retained evidence and unfinished-operation material are reference-protected,
// in-flight acquisitions are skipped instead of raced, and retained evidence
// stays verifiable after the reclaim.
func TestCachePruneProtectsPinnedRetainedAndInFlight(t *testing.T) {
	ctx := context.Background()
	store := newCacheWorkspace(t)
	cache := store.Cache()

	ordinary := putObject(t, cache, "ordinary-one", nil)
	time.Sleep(2 * time.Millisecond)
	pinned := putObject(t, cache, "pinned-object", &CacheRetainOwner{Kind: RetainPinned, Owner: "PIN-9"})
	time.Sleep(2 * time.Millisecond)
	evidence := putObject(t, cache, "evidence-object", &CacheRetainOwner{Kind: RetainEvidence, Owner: "CAP-9"})
	time.Sleep(2 * time.Millisecond)
	operation := putObject(t, cache, "operation-object", &CacheRetainOwner{Kind: RetainOperation, Owner: "OP-9"})
	time.Sleep(2 * time.Millisecond)
	inFlight := putObject(t, cache, "in-flight-object", nil)

	// Unfinished-operation material outside the managed cache is untouchable.
	runMaterial := filepath.Join(store.Root(), "runs", "OP-9", "material.bin")
	if err := os.MkdirAll(filepath.Dir(runMaterial), 0700); err != nil {
		t.Fatal(err)
	}
	runDigest := writeSentinel(t, runMaterial, "operation material\n")
	// Retained evidence archive (blobs) must survive and stay verifiable.
	blob, err := store.PublishBlob(ctx, BlobInput{Reader: strings.NewReader("retained capture body"), MaxBytes: 1024})
	if err != nil {
		t.Fatal(err)
	}
	// Uncommitted staging is protected unless explicitly included.
	if err := os.MkdirAll(cache.stagingDir(), 0700); err != nil {
		t.Fatal(err)
	}
	staging := filepath.Join(cache.stagingDir(), "put-12345678.part")
	writeSentinel(t, staging, "unfinished download\n")

	// Simulate an acquisition in flight on one object.
	lease, err := AcquireLease(ctx, cache.locksDir(), objectLeaseName(inFlight.SHA256))
	if err != nil {
		t.Fatal(err)
	}

	report, err := cache.Prune(ctx, PrunePolicy{TargetBytes: 0})
	if err != nil {
		t.Fatalf("Prune() error = %v", err)
	}
	lease.Close()

	if report.RemovedObjects != 1 || report.ReclaimedBytes != ordinary.Bytes {
		t.Fatalf("prune report = %+v", report)
	}
	if report.SkippedProtected != 3 {
		t.Fatalf("skippedProtected = %d, want 3", report.SkippedProtected)
	}
	if report.SkippedInUse != 1 {
		t.Fatalf("skippedInUse = %d, want 1", report.SkippedInUse)
	}
	if report.SkippedUncommitted != 1 || report.RemovedUncommitted != 0 {
		t.Fatalf("uncommitted handling = %+v", report)
	}
	if _, err := cache.Read(ctx, ordinary, 1<<20); !errors.Is(err, ErrCacheMissing) {
		t.Fatalf("ordinary object survived: %v", err)
	}
	for name, ref := range map[string]CacheRef{"pinned": pinned, "evidence": evidence, "operation": operation, "in-flight": inFlight} {
		data, err := cache.Read(ctx, ref, 1<<20)
		if err != nil {
			t.Fatalf("%s object lost to prune: %v", name, err)
		}
		if len(data) == 0 {
			t.Fatalf("%s object empty", name)
		}
	}
	if err := store.VerifyBlob(ctx, blob); err != nil {
		t.Fatalf("retained capture unreadable after prune: %v", err)
	}
	if digestOf(t, runMaterial) != runDigest {
		t.Fatal("unfinished-operation material changed")
	}
	if _, err := os.Lstat(staging); err != nil {
		t.Fatalf("uncommitted staging removed without explicit scope: %v", err)
	}

	// Explicit scope reclaims uncommitted staging that is not in flight.
	report, err = cache.Prune(ctx, PrunePolicy{TargetBytes: 0, IncludeUncommitted: true})
	if err != nil || report.RemovedUncommitted != 1 {
		t.Fatalf("uncommitted prune = %+v, %v", report, err)
	}
	if _, err := os.Lstat(staging); !errors.Is(err, os.ErrNotExist) {
		t.Fatal("staging survived explicit uncommitted prune")
	}
}

// CON-07: acquisition and prune coordinate; a reader either sees the complete
// verified object or a clean cache-object-missing error, never a torn read.
func TestCachePruneConcurrentAcquisitionHasNoDeletionRace(t *testing.T) {
	ctx := context.Background()
	store := newCacheWorkspace(t)
	cache := store.Cache()
	ref := putObject(t, cache, "hot object content for racing", nil)

	const rounds = 200
	var wg sync.WaitGroup
	start := make(chan struct{})
	errCh := make(chan error, rounds+8)
	for worker := 0; worker < 2; worker++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			<-start
			for i := 0; i < rounds; i++ {
				data, err := cache.Read(ctx, ref, 1<<20)
				switch {
				case err == nil:
					if string(data) != "hot object content for racing" {
						errCh <- fmt.Errorf("torn read: %q", data)
						return
					}
				case errors.Is(err, ErrCacheMissing):
					// Clean, precise outcome after deletion.
				default:
					errCh <- fmt.Errorf("unexpected acquisition error: %w", err)
					return
				}
			}
		}()
	}
	wg.Add(1)
	go func() {
		defer wg.Done()
		<-start
		for i := 0; i < rounds; i++ {
			if _, err := cache.Prune(ctx, PrunePolicy{TargetBytes: 0}); err != nil {
				errCh <- err
				return
			}
		}
	}()
	close(start)
	wg.Wait()
	close(errCh)
	for err := range errCh {
		t.Fatal(err)
	}
	// Re-publication after the race restores one verified object.
	again := putObject(t, cache, "hot object content for racing", nil)
	if again.SHA256 != ref.SHA256 {
		t.Fatalf("content address changed: %+v vs %+v", again, ref)
	}
}

// CON-05: concurrent publishers of the same immutable object coalesce onto one
// verified copy and every publisher returns the same identity.
func TestCachePutSingleWriterCoalesces(t *testing.T) {
	ctx := context.Background()
	store := newCacheWorkspace(t)
	cache := store.Cache()
	const workers = 4
	var wg sync.WaitGroup
	start := make(chan struct{})
	refs := make([]CacheRef, workers)
	errs := make([]error, workers)
	for i := 0; i < workers; i++ {
		wg.Add(1)
		go func(index int) {
			defer wg.Done()
			<-start
			refs[index], errs[index] = cache.Put(ctx, CacheInput{
				Reader: strings.NewReader("shared immutable payload"), MaxBytes: 1 << 20,
			})
		}(i)
	}
	close(start)
	wg.Wait()
	for i := 0; i < workers; i++ {
		if errs[i] != nil {
			t.Fatalf("worker %d error = %v", i, errs[i])
		}
		if refs[i] != refs[0] {
			t.Fatalf("publishers disagree: %+v vs %+v", refs[i], refs[0])
		}
	}
	status, err := cache.Status(ctx)
	if err != nil || status.ObjectCount != 1 {
		t.Fatalf("Status() = %+v, %v", status, err)
	}
	// An interrupted publication leaves no object behind and later publishes
	// still succeed (writer-death recovery).
	cancelCtx, cancel := context.WithCancel(ctx)
	cancel()
	if _, err := cache.Put(cancelCtx, CacheInput{Reader: strings.NewReader("never"), MaxBytes: 1 << 20}); !errors.Is(err, context.Canceled) {
		t.Fatalf("cancelled Put() error = %v, want context canceled", err)
	}
	if data, err := cache.Read(ctx, refs[0], 1<<20); err != nil || string(data) != "shared immutable payload" {
		t.Fatalf("Read() = %q, %v", data, err)
	}
}

func TestCacheBudgetAndInputLimits(t *testing.T) {
	ctx := context.Background()
	store := newCacheWorkspace(t)
	cache := store.Cache()
	if _, err := WriteConfig(ctx, store.Root(), Config{Schema: ConfigSchema, Cache: CacheBudget{MaxBytes: 100}, Download: DownloadBudget{}}); err != nil {
		t.Fatal(err)
	}
	first, err := cache.Put(ctx, CacheInput{Reader: strings.NewReader(strings.Repeat("a", 60)), MaxBytes: 1 << 20})
	if err != nil {
		t.Fatal(err)
	}
	if _, err := cache.Put(ctx, CacheInput{Reader: strings.NewReader(strings.Repeat("b", 60)), MaxBytes: 1 << 20}); !errors.Is(err, ErrCacheBudget) {
		t.Fatalf("over-budget Put() error = %v, want %v", err, ErrCacheBudget)
	}
	if _, err := cache.Put(ctx, CacheInput{Reader: strings.NewReader("oversize"), MaxBytes: 4}); !errors.Is(err, ErrCacheLimit) {
		t.Fatalf("over-limit Put() error = %v, want %v", err, ErrCacheLimit)
	}
	if _, err := cache.Put(ctx, CacheInput{Reader: strings.NewReader("x"), MaxBytes: 8, ExpectedSHA256: strings.Repeat("0", 64)}); !errors.Is(err, ErrCacheIntegrity) {
		t.Fatalf("bad digest Put() error = %v, want %v", err, ErrCacheIntegrity)
	}
	if _, err := cache.Prune(ctx, PrunePolicy{TargetBytes: 0}); err != nil {
		t.Fatal(err)
	}
	if _, err := cache.Put(ctx, CacheInput{Reader: strings.NewReader(strings.Repeat("b", 60)), MaxBytes: 1 << 20}); err != nil {
		t.Fatalf("Put() after prune error = %v", err)
	}
	if _, err := cache.Read(ctx, first, 4); !errors.Is(err, ErrCacheLimit) {
		t.Fatalf("Read() over maxBytes error = %v, want %v", err, ErrCacheLimit)
	}
}

func TestCachePruneLeavesForeignFilesAndRequiresExplicitScope(t *testing.T) {
	ctx := context.Background()
	store := newCacheWorkspace(t)
	cache := store.Cache()
	// PKG-06: pruning only touches files it owns. Extra user files inside the
	// managed tree are preserved and reported, and deleting uncommitted or
	// foreign material always requires explicit scope.
	foreign := filepath.Join(cache.objectsDir(), "zz", "user notes.txt")
	writeSentinel(t, foreign, "user file inside cache\n")
	ordinary := putObject(t, cache, "ordinary for foreign test", nil)

	report, err := cache.Prune(ctx, PrunePolicy{TargetBytes: 0})
	if err != nil {
		t.Fatal(err)
	}
	if report.SkippedForeign < 1 {
		t.Fatalf("foreign file not reported: %+v", report)
	}
	if digestOf(t, foreign) == "" {
		t.Fatal("foreign file lost")
	}
	if _, err := cache.Read(ctx, ordinary, 1<<20); !errors.Is(err, ErrCacheMissing) {
		t.Fatalf("ordinary object survived: %v", err)
	}
}

func TestCacheRetainAndReleaseRestoreOrdinaryState(t *testing.T) {
	ctx := context.Background()
	store := newCacheWorkspace(t)
	cache := store.Cache()
	ref := putObject(t, cache, "retained then released", nil)
	owner := CacheRetainOwner{Kind: RetainOperation, Owner: "OP-77"}
	if err := cache.Retain(ctx, ref, owner); err != nil {
		t.Fatal(err)
	}
	report, err := cache.Prune(ctx, PrunePolicy{TargetBytes: 0})
	if err != nil || report.RemovedObjects != 0 || report.SkippedProtected != 1 {
		t.Fatalf("Prune() = %+v, %v", report, err)
	}
	if err := cache.Release(ctx, ref, owner); err != nil {
		t.Fatal(err)
	}
	report, err = cache.Prune(ctx, PrunePolicy{TargetBytes: 0})
	if err != nil || report.RemovedObjects != 1 {
		t.Fatalf("Prune() = %+v, %v", report, err)
	}
	status, err := cache.Status(ctx)
	if err != nil || status.OrphanMarkers != 0 || status.ProtectedCount != 0 {
		t.Fatalf("Status() = %+v, %v", status, err)
	}
}
