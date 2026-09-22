package vault

import (
	"context"
	"crypto/rand"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"time"
)

var (
	// ErrCacheIntegrity rejects content that does not match its object name.
	ErrCacheIntegrity = errors.New("vault.cache_integrity")
	// ErrCacheLimit rejects input larger than the caller's byte budget.
	ErrCacheLimit = errors.New("vault.cache_limit")
	// ErrCacheBudget reports that publishing would exceed the configured
	// managed cache limit; pruning ordinary cache first reclaims space.
	ErrCacheBudget = errors.New("vault.cache_budget_exceeded")
	// ErrCacheMissing reports an object that is not in the managed cache.
	ErrCacheMissing = errors.New("vault.cache_object_missing")
	// ErrCacheProtection rejects invalid retention records.
	ErrCacheProtection = errors.New("vault.cache_protection")
)

// Cache retention kinds. Only ordinary, unprotected cache is reclaimable.
const (
	RetainPinned    = "pinned"
	RetainEvidence  = "evidence"
	RetainOperation = "operation"
)

// CacheRef names a managed cache object by its content hash.
type CacheRef struct {
	SHA256 string `json:"sha256"`
	Bytes  int64  `json:"bytes"`
}

// CacheRetainOwner is one reference-protected owner of a cache object: a
// pinned set, retained evidence, or unfinished-operation material.
type CacheRetainOwner struct {
	Kind       string    `json:"kind"`
	Owner      string    `json:"owner"`
	RetainedAt time.Time `json:"retainedAt"`
}

// CacheRetain lists the protective owners recorded for one object.
type CacheRetain struct {
	SHA256 string             `json:"sha256"`
	Owners []CacheRetainOwner `json:"owners"`
}

// CacheInput publishes one immutable cache object. Retain, when set, is
// committed under the same object lock as the content, so a concurrent prune
// can never observe the fresh object as unprotected.
type CacheInput struct {
	Reader         io.Reader
	MaxBytes       int64
	ExpectedSHA256 string
	Retain         *CacheRetainOwner
}

// CacheStatus reports managed cache accounting and protection visibility.
type CacheStatus struct {
	ObjectBytes      int64          `json:"objectBytes"`
	ObjectCount      int            `json:"objectCount"`
	UncommittedBytes int64          `json:"uncommittedBytes"`
	UncommittedCount int            `json:"uncommittedCount"`
	ProtectedCount   int            `json:"protectedCount"`
	ProtectedByKind  map[string]int `json:"protectedByKind"`
	Protected        []CacheRetain  `json:"protected,omitempty"`
	OrphanMarkers    int            `json:"orphanMarkers"`
	ForeignCount     int            `json:"foreignCount"`
	LimitBytes       int64          `json:"limitBytes"`
	WithinLimit      bool           `json:"withinLimit"`
}

// CacheVerifyReport is a read-only integrity result. Nothing is repaired or
// deleted; corrupt or missing objects are reported for an explicit decision.
type CacheVerifyReport struct {
	OK      bool     `json:"ok"`
	Checked int      `json:"checked"`
	Bytes   int64    `json:"bytes"`
	Missing []string `json:"missing"`
	Corrupt []string `json:"corrupt"`
	Errors  []string `json:"errors"`
}

// PrunePolicy bounds one reclaim pass. TargetBytes is the desired managed
// object total afterwards (zero reclaims all ordinary cache); MaxObjects caps
// deletions (zero is uncapped but still cancellable). Uncommitted staging is
// only reclaimed with explicit IncludeUncommitted, so unfinished-operation
// material survives ordinary pruning.
type PrunePolicy struct {
	TargetBytes        int64
	MaxObjects         int
	IncludeUncommitted bool
	DryRun             bool
}

// PruneReport is an honest partial-capable reclaim result. Complete is false
// whenever a bound stopped the pass early or the target was not reached.
type PruneReport struct {
	TargetBytes        int64 `json:"targetBytes"`
	BeforeBytes        int64 `json:"beforeBytes"`
	AfterBytes         int64 `json:"afterBytes"`
	ReclaimedBytes     int64 `json:"reclaimedBytes"`
	RemovedObjects     int   `json:"removedObjects"`
	RemovedUncommitted int   `json:"removedUncommitted"`
	SkippedProtected   int   `json:"skippedProtected"`
	SkippedInUse       int   `json:"skippedInUse"`
	SkippedUncommitted int   `json:"skippedUncommitted"`
	SkippedForeign     int   `json:"skippedForeign"`
	Truncated          bool  `json:"truncated"`
	Complete           bool  `json:"complete"`
	DryRun             bool  `json:"dryRun"`
}

// CacheManager owns the reclaimable managed cache under <root>/cache.
// Persistent blobs, captures and runs are outside its scope and can never be
// deleted by pruning.
type CacheManager struct{ store *Store }

func (s *Store) Cache() *CacheManager { return &CacheManager{store: s} }

func (c *CacheManager) objectsDir() string { return filepath.Join(c.store.Root(), "cache", "objects") }
func (c *CacheManager) retainDir() string  { return filepath.Join(c.store.Root(), "cache", "retain") }
func (c *CacheManager) stagingDir() string { return filepath.Join(c.store.Root(), "cache", "tmp") }
func (c *CacheManager) locksDir() string   { return filepath.Join(c.store.Root(), "locks") }

func (c *CacheManager) objectPath(digest string) string {
	return filepath.Join(c.objectsDir(), digest[:2], digest[2:])
}

func (c *CacheManager) retainPath(digest string) string {
	return filepath.Join(c.retainDir(), digest+".json")
}

func objectLeaseName(digest string) string { return "cache:obj:" + digest }
func stageLeaseName(id string) string      { return "cache:stage:" + id }

// Put publishes one verified immutable object. Concurrent publishers of the
// same content serialize on the object lock and coalesce onto the single
// verified copy; a waiting publisher reuses it instead of writing again.
func (c *CacheManager) Put(ctx context.Context, input CacheInput) (CacheRef, error) {
	var zero CacheRef
	if input.Reader == nil || input.MaxBytes <= 0 || input.MaxBytes == 1<<63-1 {
		return zero, errors.New("vault: invalid cache input")
	}
	if input.ExpectedSHA256 != "" && !validDigest(input.ExpectedSHA256) {
		return zero, ErrCacheIntegrity
	}
	if input.Retain != nil {
		if err := validateRetainOwner(*input.Retain); err != nil {
			return zero, err
		}
	}
	if err := ctx.Err(); err != nil {
		return zero, err
	}
	stageID, err := randomToken()
	if err != nil {
		return zero, err
	}
	stageLease, err := AcquireLease(ctx, c.locksDir(), stageLeaseName(stageID))
	if err != nil {
		return zero, err
	}
	defer stageLease.Close()
	if err := os.MkdirAll(c.stagingDir(), 0700); err != nil {
		return zero, err
	}
	staging := filepath.Join(c.stagingDir(), "put-"+stageID+".part")
	f, err := os.OpenFile(staging, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0600)
	if err != nil {
		return zero, err
	}
	defer os.Remove(staging)
	defer f.Close()
	hash := sha256.New()
	count, err := io.Copy(io.MultiWriter(f, hash), io.LimitReader(&interruptibleReader{ctx, input.Reader}, input.MaxBytes+1))
	if err != nil {
		return zero, err
	}
	if count > input.MaxBytes {
		return zero, ErrCacheLimit
	}
	ref := CacheRef{SHA256: hex.EncodeToString(hash.Sum(nil)), Bytes: count}
	if input.ExpectedSHA256 != "" && input.ExpectedSHA256 != ref.SHA256 {
		return zero, ErrCacheIntegrity
	}
	if err := f.Sync(); err != nil {
		return zero, err
	}
	if err := f.Close(); err != nil {
		return zero, err
	}
	objLease, err := AcquireLease(ctx, c.locksDir(), objectLeaseName(ref.SHA256))
	if err != nil {
		return zero, err
	}
	defer objLease.Close()
	destination := c.objectPath(ref.SHA256)
	if _, err := os.Lstat(destination); err == nil {
		if err := c.verifyObjectFile(ctx, ref); err == nil {
			// Another publisher already committed the verified object.
			if input.Retain != nil {
				return ref, c.writeRetainLocked(ctx, ref.SHA256, *input.Retain)
			}
			return ref, nil
		}
		// The cached copy is damaged; the staged bytes are verified, replace it.
		if err := os.Remove(destination); err != nil && !errors.Is(err, os.ErrNotExist) {
			return zero, err
		}
	} else if !errors.Is(err, os.ErrNotExist) {
		return zero, err
	}
	if err := c.checkBudget(ctx, count); err != nil {
		return zero, err
	}
	if err := os.MkdirAll(filepath.Dir(destination), 0700); err != nil {
		return zero, err
	}
	if err := ctx.Err(); err != nil {
		return zero, err
	}
	if err := os.Rename(staging, destination); err != nil {
		return zero, err
	}
	if input.Retain != nil {
		return ref, c.writeRetainLocked(ctx, ref.SHA256, *input.Retain)
	}
	return ref, nil
}

// Read returns one fully verified object. The object lock is held for the
// whole acquisition, so a concurrent prune can never delete it mid-read.
func (c *CacheManager) Read(ctx context.Context, ref CacheRef, maxBytes int64) ([]byte, error) {
	if !validDigest(ref.SHA256) || ref.Bytes < 0 {
		return nil, ErrCacheIntegrity
	}
	if maxBytes < 0 || ref.Bytes > maxBytes || ref.Bytes == 1<<63-1 {
		return nil, ErrCacheLimit
	}
	lease, err := AcquireLease(ctx, c.locksDir(), objectLeaseName(ref.SHA256))
	if err != nil {
		return nil, err
	}
	defer lease.Close()
	f, err := os.Open(c.objectPath(ref.SHA256))
	if errors.Is(err, os.ErrNotExist) {
		return nil, fmt.Errorf("%w: %s", ErrCacheMissing, ref.SHA256)
	}
	if err != nil {
		return nil, err
	}
	defer f.Close()
	stat, err := f.Stat()
	if err != nil {
		return nil, err
	}
	if !stat.Mode().IsRegular() || stat.Size() != ref.Bytes {
		return nil, ErrCacheIntegrity
	}
	data, err := io.ReadAll(io.LimitReader(&interruptibleReader{ctx, f}, ref.Bytes+1))
	if err != nil {
		return nil, err
	}
	digest := sha256.Sum256(data)
	if int64(len(data)) != ref.Bytes || hex.EncodeToString(digest[:]) != ref.SHA256 {
		return nil, ErrCacheIntegrity
	}
	return data, nil
}

// Stream hands the object bytes to fn while holding the object lock, then
// verifies the consumed content against the object name. Callers that need
// verified bytes before use should call Read instead.
func (c *CacheManager) Stream(ctx context.Context, ref CacheRef, fn func(io.Reader) error) error {
	if !validDigest(ref.SHA256) || ref.Bytes < 0 || fn == nil {
		return ErrCacheIntegrity
	}
	lease, err := AcquireLease(ctx, c.locksDir(), objectLeaseName(ref.SHA256))
	if err != nil {
		return err
	}
	defer lease.Close()
	f, err := os.Open(c.objectPath(ref.SHA256))
	if errors.Is(err, os.ErrNotExist) {
		return fmt.Errorf("%w: %s", ErrCacheMissing, ref.SHA256)
	}
	if err != nil {
		return err
	}
	defer f.Close()
	stat, err := f.Stat()
	if err != nil {
		return err
	}
	if !stat.Mode().IsRegular() || stat.Size() != ref.Bytes {
		return ErrCacheIntegrity
	}
	hash := sha256.New()
	wrapped := &interruptibleReader{ctx, io.TeeReader(f, hash)}
	if err := fn(wrapped); err != nil {
		return err
	}
	if _, err := io.Copy(io.Discard, wrapped); err != nil {
		return err
	}
	if hex.EncodeToString(hash.Sum(nil)) != ref.SHA256 {
		return ErrCacheIntegrity
	}
	return nil
}

// Retain adds one reference-protected owner to an object. It may run before
// the object is published so acquisition-side protection cannot race a prune;
// markers without an object are reported, never silently dropped.
func (c *CacheManager) Retain(ctx context.Context, ref CacheRef, owner CacheRetainOwner) error {
	if !validDigest(ref.SHA256) {
		return ErrCacheIntegrity
	}
	if err := validateRetainOwner(owner); err != nil {
		return err
	}
	lease, err := AcquireLease(ctx, c.locksDir(), objectLeaseName(ref.SHA256))
	if err != nil {
		return err
	}
	defer lease.Close()
	return c.writeRetainLocked(ctx, ref.SHA256, owner)
}

// Release drops one owner. The object itself becomes ordinary cache again
// only when no owners remain.
func (c *CacheManager) Release(ctx context.Context, ref CacheRef, owner CacheRetainOwner) error {
	if !validDigest(ref.SHA256) {
		return ErrCacheIntegrity
	}
	lease, err := AcquireLease(ctx, c.locksDir(), objectLeaseName(ref.SHA256))
	if err != nil {
		return err
	}
	defer lease.Close()
	retain, err := c.readRetainLocked(ref.SHA256)
	if err != nil {
		return err
	}
	kept := make([]CacheRetainOwner, 0, len(retain.Owners))
	for _, existing := range retain.Owners {
		if existing.Kind != owner.Kind || existing.Owner != owner.Owner {
			kept = append(kept, existing)
		}
	}
	if len(kept) == len(retain.Owners) {
		return nil
	}
	path := c.retainPath(ref.SHA256)
	if len(kept) == 0 {
		err := os.Remove(path)
		if errors.Is(err, os.ErrNotExist) {
			return nil
		}
		return err
	}
	return c.writeRetainFileLocked(ref.SHA256, kept)
}

// Status is a read-only accounting walk with protection visibility.
func (c *CacheManager) Status(ctx context.Context) (CacheStatus, error) {
	status := CacheStatus{ProtectedByKind: map[string]int{}, WithinLimit: true}
	if err := ctx.Err(); err != nil {
		return status, err
	}
	err := walkFiles(c.objectsDir(), func(path string, entry os.DirEntry) error {
		digest, ok := objectDigestFromPath(c.objectsDir(), path)
		if !ok {
			status.ForeignCount++
			return nil
		}
		info, err := entry.Info()
		if err != nil {
			return err
		}
		status.ObjectBytes += info.Size()
		status.ObjectCount++
		_ = digest
		return nil
	})
	if err != nil {
		return status, err
	}
	err = walkFiles(c.stagingDir(), func(path string, entry os.DirEntry) error {
		info, err := entry.Info()
		if err != nil {
			return err
		}
		status.UncommittedBytes += info.Size()
		status.UncommittedCount++
		return nil
	})
	if err != nil {
		return status, err
	}
	err = walkFiles(c.retainDir(), func(path string, entry os.DirEntry) error {
		if !strings.HasSuffix(entry.Name(), ".json") {
			status.ForeignCount++
			return nil
		}
		digest := strings.TrimSuffix(entry.Name(), ".json")
		if !validDigest(digest) {
			status.ForeignCount++
			return nil
		}
		retain, err := c.readRetainLocked(digest)
		if err != nil {
			return err
		}
		if len(retain.Owners) == 0 {
			return nil
		}
		if _, err := os.Lstat(c.objectPath(digest)); errors.Is(err, os.ErrNotExist) {
			status.OrphanMarkers++
			return nil
		} else if err != nil {
			return err
		}
		status.ProtectedCount++
		kinds := map[string]bool{}
		for _, owner := range retain.Owners {
			kinds[owner.Kind] = true
		}
		for kind := range kinds {
			status.ProtectedByKind[kind]++
		}
		if len(status.Protected) < 100 {
			status.Protected = append(status.Protected, retain)
		}
		return nil
	})
	if err != nil {
		return status, err
	}
	config, err := ReadConfig(ctx, c.store.Root())
	if err != nil {
		return status, err
	}
	status.LimitBytes = config.Cache.MaxBytes
	status.WithinLimit = config.Cache.MaxBytes <= 0 || status.ObjectBytes+status.UncommittedBytes <= config.Cache.MaxBytes
	sort.Slice(status.Protected, func(i, j int) bool { return status.Protected[i].SHA256 < status.Protected[j].SHA256 })
	return status, nil
}

// Verify checks every managed object against its content hash. It is strictly
// read-only and never repairs or deletes.
func (c *CacheManager) Verify(ctx context.Context) (CacheVerifyReport, error) {
	report := CacheVerifyReport{OK: true, Missing: []string{}, Corrupt: []string{}, Errors: []string{}}
	err := walkFiles(c.objectsDir(), func(path string, entry os.DirEntry) error {
		digest, ok := objectDigestFromPath(c.objectsDir(), path)
		if !ok {
			report.Errors = append(report.Errors, "unexpected cache file: "+filepath.Base(path))
			return nil
		}
		info, err := entry.Info()
		if err != nil {
			report.Errors = append(report.Errors, err.Error())
			return nil
		}
		report.Checked++
		report.Bytes += info.Size()
		if err := c.verifyObjectFile(ctx, CacheRef{SHA256: digest, Bytes: info.Size()}); err != nil {
			report.Corrupt = append(report.Corrupt, digest)
		}
		return nil
	})
	if err != nil {
		report.Errors = append(report.Errors, err.Error())
	}
	err = walkFiles(c.retainDir(), func(path string, entry os.DirEntry) error {
		if !strings.HasSuffix(entry.Name(), ".json") {
			return nil
		}
		digest := strings.TrimSuffix(entry.Name(), ".json")
		retain, err := c.readRetainLocked(digest)
		if err != nil {
			report.Errors = append(report.Errors, err.Error())
			return nil
		}
		if len(retain.Owners) == 0 {
			return nil
		}
		if _, err := os.Lstat(c.objectPath(digest)); errors.Is(err, os.ErrNotExist) {
			report.Missing = append(report.Missing, digest)
		} else if err != nil {
			report.Errors = append(report.Errors, err.Error())
		}
		return nil
	})
	if err != nil {
		report.Errors = append(report.Errors, err.Error())
	}
	sort.Strings(report.Missing)
	sort.Strings(report.Corrupt)
	sort.Strings(report.Errors)
	report.OK = len(report.Missing) == 0 && len(report.Corrupt) == 0 && len(report.Errors) == 0
	return report, nil
}

// Prune reclaims ordinary cache only. Every candidate is decided and deleted
// under its object lock, so an acquisition in flight is skipped instead of
// raced, and pinned, retained-evidence and unfinished-operation references are
// never deleted. The pass is bounded and cancellable and reports honestly.
func (c *CacheManager) Prune(ctx context.Context, policy PrunePolicy) (PruneReport, error) {
	report := PruneReport{TargetBytes: policy.TargetBytes, DryRun: policy.DryRun}
	type candidate struct {
		digest string
		path   string
		size   int64
		mod    int64
	}
	candidates := make([]candidate, 0)
	foreign := 0
	err := walkFiles(c.objectsDir(), func(path string, entry os.DirEntry) error {
		digest, ok := objectDigestFromPath(c.objectsDir(), path)
		if !ok {
			foreign++
			return nil
		}
		info, err := entry.Info()
		if err != nil {
			return err
		}
		candidates = append(candidates, candidate{digest: digest, path: path, size: info.Size(), mod: info.ModTime().UnixNano()})
		return nil
	})
	if err != nil {
		return report, err
	}
	report.SkippedForeign += foreign
	sort.Slice(candidates, func(i, j int) bool { return candidates[i].mod < candidates[j].mod })
	currentBytes := int64(0)
	for _, item := range candidates {
		currentBytes += item.size
	}
	uncommitted := int64(0)
	uncommittedCount := 0
	err = walkFiles(c.stagingDir(), func(path string, entry os.DirEntry) error {
		info, err := entry.Info()
		if err != nil {
			return err
		}
		uncommitted += info.Size()
		uncommittedCount++
		return nil
	})
	if err != nil {
		return report, err
	}
	report.BeforeBytes = currentBytes + uncommitted

	for _, item := range candidates {
		if err := ctx.Err(); err != nil {
			report.AfterBytes = currentBytes + uncommitted
			report.Complete = false
			return report, err
		}
		if currentBytes <= policy.TargetBytes {
			break
		}
		if policy.MaxObjects > 0 && report.RemovedObjects >= policy.MaxObjects {
			report.Truncated = true
			break
		}
		lease, err := TryAcquireLease(ctx, c.locksDir(), objectLeaseName(item.digest))
		if err != nil {
			if errors.Is(err, ErrLeaseBusy) {
				report.SkippedInUse++
				continue
			}
			return report, err
		}
		skip, err := c.protectedLocked(item.digest)
		if err != nil {
			lease.Close()
			return report, err
		}
		if skip {
			report.SkippedProtected++
			lease.Close()
			continue
		}
		if !policy.DryRun {
			if err := os.Remove(item.path); err != nil && !errors.Is(err, os.ErrNotExist) {
				lease.Close()
				return report, err
			}
		}
		currentBytes -= item.size
		report.RemovedObjects++
		report.ReclaimedBytes += item.size
		lease.Close()
	}

	if policy.IncludeUncommitted {
		type staged struct {
			id   string
			path string
			size int64
		}
		items := make([]staged, 0)
		err = walkFiles(c.stagingDir(), func(path string, entry os.DirEntry) error {
			info, err := entry.Info()
			if err != nil {
				return err
			}
			name := entry.Name()
			id := strings.TrimSuffix(strings.TrimPrefix(name, "put-"), ".part")
			if !strings.HasPrefix(name, "put-") || !strings.HasSuffix(name, ".part") {
				report.SkippedForeign++
				return nil
			}
			items = append(items, staged{id: id, path: path, size: info.Size()})
			return nil
		})
		if err != nil {
			return report, err
		}
		for _, item := range items {
			if err := ctx.Err(); err != nil {
				report.AfterBytes = currentBytes + uncommitted
				report.Complete = false
				return report, err
			}
			lease, err := TryAcquireLease(ctx, c.locksDir(), stageLeaseName(item.id))
			if err != nil {
				if errors.Is(err, ErrLeaseBusy) {
					report.SkippedInUse++
					continue
				}
				return report, err
			}
			if !policy.DryRun {
				if err := os.Remove(item.path); err != nil && !errors.Is(err, os.ErrNotExist) {
					lease.Close()
					return report, err
				}
			}
			uncommitted -= item.size
			report.RemovedUncommitted++
			report.ReclaimedBytes += item.size
			lease.Close()
		}
	} else {
		report.SkippedUncommitted += uncommittedCount
	}

	report.AfterBytes = currentBytes + uncommitted
	report.Complete = !report.Truncated && currentBytes <= policy.TargetBytes
	return report, nil
}

func (c *CacheManager) protectedLocked(digest string) (bool, error) {
	retain, err := c.readRetainLocked(digest)
	if err != nil {
		return false, err
	}
	return len(retain.Owners) > 0, nil
}

func (c *CacheManager) readRetainLocked(digest string) (CacheRetain, error) {
	var retain CacheRetain
	raw, err := os.ReadFile(c.retainPath(digest))
	if errors.Is(err, os.ErrNotExist) {
		return CacheRetain{SHA256: digest}, nil
	}
	if err != nil {
		return retain, err
	}
	if len(raw) > 65536 || !json.Valid(raw) {
		return retain, fmt.Errorf("%w: corrupt retain marker for %s", ErrCacheProtection, digest)
	}
	if err := json.Unmarshal(raw, &retain); err != nil {
		return retain, fmt.Errorf("%w: %v", ErrCacheProtection, err)
	}
	if retain.SHA256 != digest {
		return retain, fmt.Errorf("%w: retain marker identity", ErrCacheProtection)
	}
	for _, owner := range retain.Owners {
		if err := validateRetainOwner(owner); err != nil {
			return retain, err
		}
	}
	return retain, nil
}

func (c *CacheManager) writeRetainLocked(ctx context.Context, digest string, owner CacheRetainOwner) error {
	retain, err := c.readRetainLocked(digest)
	if err != nil {
		return err
	}
	for _, existing := range retain.Owners {
		if existing.Kind == owner.Kind && existing.Owner == owner.Owner {
			return nil
		}
	}
	if owner.RetainedAt.IsZero() {
		owner.RetainedAt = time.Now().UTC()
	}
	return c.writeRetainFileLocked(digest, append(retain.Owners, owner))
}

func (c *CacheManager) writeRetainFileLocked(digest string, owners []CacheRetainOwner) error {
	raw, err := json.MarshalIndent(CacheRetain{SHA256: digest, Owners: owners}, "", "  ")
	if err != nil {
		return err
	}
	if err := os.MkdirAll(c.retainDir(), 0700); err != nil {
		return err
	}
	return writeDurableReplace(c.retainDir(), digest+".json", append(raw, '\n'))
}

// verifyObjectFile checks one object against its content-addressed name.
func (c *CacheManager) verifyObjectFile(ctx context.Context, ref CacheRef) error {
	f, err := os.Open(c.objectPath(ref.SHA256))
	if err != nil {
		return err
	}
	defer f.Close()
	stat, err := f.Stat()
	if err != nil {
		return err
	}
	if !stat.Mode().IsRegular() || stat.Size() != ref.Bytes {
		return ErrCacheIntegrity
	}
	hash := sha256.New()
	count, err := io.Copy(hash, &interruptibleReader{ctx, f})
	if err != nil {
		return err
	}
	if count != ref.Bytes || hex.EncodeToString(hash.Sum(nil)) != ref.SHA256 {
		return ErrCacheIntegrity
	}
	return nil
}

func (c *CacheManager) checkBudget(ctx context.Context, incoming int64) error {
	config, err := ReadConfig(ctx, c.store.Root())
	if err != nil {
		return err
	}
	if config.Cache.MaxBytes <= 0 {
		return nil
	}
	current := int64(0)
	err = walkFiles(c.objectsDir(), func(path string, entry os.DirEntry) error {
		if _, ok := objectDigestFromPath(c.objectsDir(), path); !ok {
			return nil
		}
		info, err := entry.Info()
		if err != nil {
			return err
		}
		current += info.Size()
		return nil
	})
	if err != nil {
		return err
	}
	if current+incoming > config.Cache.MaxBytes {
		return fmt.Errorf("%w: %d + %d exceeds %d", ErrCacheBudget, current, incoming, config.Cache.MaxBytes)
	}
	return nil
}

func validateRetainOwner(owner CacheRetainOwner) error {
	switch owner.Kind {
	case RetainPinned, RetainEvidence, RetainOperation:
	default:
		return fmt.Errorf("%w: unknown kind %q", ErrCacheProtection, owner.Kind)
	}
	if owner.Owner == "" || len(owner.Owner) > 128 {
		return fmt.Errorf("%w: invalid owner", ErrCacheProtection)
	}
	for _, r := range owner.Owner {
		if r < 0x20 || r == 0x7f {
			return fmt.Errorf("%w: invalid owner", ErrCacheProtection)
		}
	}
	return nil
}

// objectDigestFromPath reassembles the content name from its fan-out path and
// rejects anything that is not a well-formed managed object name.
func objectDigestFromPath(root, path string) (string, bool) {
	rel, err := filepath.Rel(root, path)
	if err != nil || strings.HasPrefix(rel, "..") {
		return "", false
	}
	parts := strings.Split(filepath.ToSlash(rel), "/")
	if len(parts) != 2 || len(parts[0]) != 2 || len(parts[1]) != 62 {
		return "", false
	}
	digest := parts[0] + parts[1]
	return digest, validDigest(digest)
}

func walkFiles(root string, visit func(path string, entry os.DirEntry) error) error {
	err := filepath.WalkDir(root, func(path string, entry os.DirEntry, walkErr error) error {
		if walkErr != nil {
			if errors.Is(walkErr, os.ErrNotExist) {
				return nil
			}
			return walkErr
		}
		if entry.IsDir() {
			return nil
		}
		return visit(path, entry)
	})
	if errors.Is(err, os.ErrNotExist) {
		return nil
	}
	return err
}

func randomToken() (string, error) {
	var raw [8]byte
	if _, err := rand.Read(raw[:]); err != nil {
		return "", err
	}
	return hex.EncodeToString(raw[:]), nil
}
