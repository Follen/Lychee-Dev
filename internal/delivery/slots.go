package delivery

import (
	"context"
	"crypto/sha256"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/vault"
)

const slotMarker = ".lycheedev-slots.json"
const slotPoolSchema = "lycheedev.slots.v3"
const previousSlotPoolSchema = "lycheedev.slots.v2"
const legacySlotPoolSchema = "lycheedev.slots.v1"
const slotPoolMigrationSchema = "lycheedev.slots.migration.v3"
const slotPoolManifestLimit = 256 << 10

const inertSlot = "LycheeDevSlotEnvelope = nil\n"

var ErrSlotReserved = errors.New("delivery.slot_reserved")

type SlotFile struct {
	PayloadHash    string `json:"payloadHash"`
	PendingHash    string `json:"pendingHash,omitempty"`
	Nonce          string `json:"nonce,omitempty"`
	Runtime        string `json:"runtime,omitempty"`
	Consumer       string `json:"consumer,omitempty"`
	Consumed       bool   `json:"consumed"`
	RetiredRuntime string `json:"retiredRuntime,omitempty"`
	RetiredProcess bool   `json:"retiredProcess,omitempty"`
}
type SlotPool struct {
	SourceSchema   string     `json:"sourceSchema,omitempty"`
	PendingVersion string     `json:"pendingVersion,omitempty"`
	Schema         string     `json:"schema"`
	Version        string     `json:"version"`
	State          string     `json:"state"`
	Files          []SlotFile `json:"files"`
}

func SlotDirectory(parent string, index int) string {
	return filepath.Join(parent, fmt.Sprintf("Lychee Dev Slot %02d", index))
}
func slotDigest(b []byte) string { return fmt.Sprintf("%x", sha256.Sum256(b)) }
func slotStatic(index int, version string) map[string][]byte {
	return slotStaticForSchema(index, version, slotPoolSchema)
}
func slotStaticForSchema(index int, version, schema string) map[string][]byte {
	transport := "memory-slot-v3"
	if schema == legacySlotPoolSchema {
		transport = "memory-slot-v1"
	} else if schema == previousSlotPoolSchema {
		transport = "memory-slot-v2"
	}
	name := fmt.Sprintf("Lychee Dev Slot %02d", index)
	return map[string][]byte{
		name + ".toc": []byte(fmt.Sprintf("## Interface: 120100, 50504, 38002, 16001\n## Title: Lychee Dev input slot %02d\n## Version: %s\n## LoadOnDemand: 1\n## X-Lychee-Slot: %d\n## X-Lychee-Transport: %s\nPayload.lua\nLoader.lua\n", index, version, index, transport)),
		"Loader.lua":  []byte(fmt.Sprintf("local envelope = LycheeDevSlotEnvelope\nLycheeDevSlotEnvelope = nil\nlocal ns = LycheeDevInternal\nif ns and ns.SlotRuntime then ns.SlotRuntime.Receive(%d, envelope) end\n", index)),
	}
}

// RepairSlotBootstrap removes only the exact pre-release dependency declaration.
// Payloads, reservations and loaders are preserved. Call under the installation
// publication lease; a client must refresh its addon catalog before retrying.
// The loader is passive unless the main runtime explicitly expects this slot.
func RepairSlotBootstrap(ctx context.Context, parent, version string) error {
	pool, err := readSlotPool(parent)
	if err != nil {
		return err
	}
	if pool.Schema != slotPoolSchema || pool.State != "ready" || pool.Version != version {
		return ErrInstallation
	}
	type replacement struct {
		path string
		data []byte
	}
	var changes []replacement
	for i, entry := range pool.Files {
		dir := SlotDirectory(parent, i+1)
		if err = plainSlotDirectory(dir); err != nil {
			return err
		}
		if err = exactSlotFileCount(dir); err != nil {
			return err
		}
		for name, want := range slotStaticForSchema(i+1, version, pool.Schema) {
			path := filepath.Join(dir, name)
			actual, e := readSlotFile(path, 16384)
			if e != nil {
				return e
			}
			if slotDigest(actual) == slotDigest(want) {
				continue
			}
			old := strings.Replace(string(want), "## LoadOnDemand: 1\n", "## LoadOnDemand: 1\n## Dependencies: Lychee Dev\n", 1)
			if !strings.HasSuffix(name, ".toc") || string(actual) != old {
				return ErrInstallation
			}
			changes = append(changes, replacement{path, want})
		}
		payload, e := readSlotFile(filepath.Join(dir, "Payload.lua"), 2<<20)
		if e != nil {
			return e
		}
		if slotDigest(payload) != entry.PayloadHash || entry.PendingHash != "" {
			return ErrInstallation
		}
	}
	for _, change := range changes {
		if err = vault.ReplaceFile(ctx, change.path, change.data); err != nil {
			return err
		}
	}
	_, err = InspectSlots(ctx, parent, version)
	return err
}
func readSlotPool(parent string) (SlotPool, error) {
	var pool SlotPool
	b, err := readSlotFile(filepath.Join(parent, slotMarker), slotPoolManifestLimit)
	if err != nil {
		return pool, err
	}
	if len(b) > slotPoolManifestLimit {
		return pool, ErrInstallation
	}
	if err = json.Unmarshal(b, &pool); err != nil {
		return pool, err
	}
	if !validSlotPoolShape(pool) {
		return pool, ErrInstallation
	}
	return pool, nil
}

// Old schemas are installation metadata only. No business operation admits them.
func validSlotPoolShape(pool SlotPool) bool {
	if pool.Version == "" {
		return false
	}
	switch pool.Schema {
	case slotPoolSchema:
		return len(pool.Files) == bridge.SlotCount && pool.SourceSchema == ""
	case legacySlotPoolSchema:
		return len(pool.Files) == 64 && pool.State == "ready" && pool.SourceSchema == ""
	case previousSlotPoolSchema:
		return len(pool.Files) == bridge.SlotCount && pool.State == "ready" && pool.SourceSchema == ""
	case slotPoolMigrationSchema:
		return pool.State == "upgrading" && pool.PendingVersion != "" && (pool.SourceSchema == legacySlotPoolSchema && len(pool.Files) == 64 || (pool.SourceSchema == previousSlotPoolSchema || pool.SourceSchema == slotPoolSchema) && len(pool.Files) == bridge.SlotCount)
	}
	return false
}

func readSlotFile(path string, limit int64) ([]byte, error) {
	info, err := os.Lstat(path)
	if err != nil {
		return nil, err
	}
	if !info.Mode().IsRegular() || info.Size() > limit {
		return nil, ErrInstallation
	}
	f, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer f.Close()
	b, err := io.ReadAll(io.LimitReader(f, limit+1))
	if int64(len(b)) > limit {
		return nil, ErrInstallation
	}
	return b, err
}
func saveSlotPool(ctx context.Context, parent string, pool SlotPool) error {
	b, err := json.MarshalIndent(pool, "", "  ")
	if err != nil {
		return err
	}
	if len(b) > slotPoolManifestLimit {
		return ErrInstallation
	}
	return vault.ReplaceFile(ctx, filepath.Join(parent, slotMarker), b)
}
func plainSlotDirectory(path string) error {
	info, err := os.Lstat(path)
	if err != nil {
		return err
	}
	resolved, err := filepath.EvalSymlinks(path)
	if err != nil {
		return err
	}
	if !info.IsDir() || info.Mode()&os.ModeSymlink != 0 || resolved != path {
		return fmt.Errorf("%w: redirected slot", ErrInstallation)
	}
	return nil
}

func exactSlotFileCount(path string) error {
	entries, err := os.ReadDir(path)
	if err != nil {
		return err
	}
	if len(entries) != 3 {
		return fmt.Errorf("%w: unexpected slot files", ErrInstallation)
	}
	return nil
}

// installSlotPool is called under installation maintenance after the main
// release has been verified. Its pending receipt makes partial install resumable.
func installSlotPool(ctx context.Context, parent, version string) (SlotPool, error) {
	pool, err := readSlotPool(parent)
	if errors.Is(err, os.ErrNotExist) {
		for i := 1; i <= bridge.SlotCount; i++ {
			if _, e := os.Lstat(SlotDirectory(parent, i)); !errors.Is(e, os.ErrNotExist) {
				return pool, fmt.Errorf("%w: unmanaged slot %d", ErrInstallation, i)
			}
		}
		pool = SlotPool{Schema: slotPoolSchema, Version: version, State: "installing", Files: make([]SlotFile, bridge.SlotCount)}
		if err = saveSlotPool(ctx, parent, pool); err != nil {
			return pool, err
		}
	} else if err != nil {
		return pool, err
	} else if pool.State == "upgrading" || pool.State == "ready" && (pool.Version != version || pool.Schema != slotPoolSchema) {
		return upgradeSlotPool(ctx, parent, pool, version)
	} else if pool.State != "ready" && pool.State != "installing" {
		return pool, ErrInstallation
	}
	for i := 1; i <= len(pool.Files); i++ {
		dir := SlotDirectory(parent, i)
		if err = os.MkdirAll(dir, 0700); err != nil {
			return pool, err
		}
		if err = plainSlotDirectory(dir); err != nil {
			return pool, err
		}
		for name, want := range slotStaticForSchema(i, pool.Version, pool.Schema) {
			path := filepath.Join(dir, name)
			actual, e := readSlotFile(path, 16384)
			if e == nil {
				if slotDigest(actual) != slotDigest(want) {
					return pool, fmt.Errorf("%w: modified slot file", ErrInstallation)
				}
			} else if errors.Is(e, os.ErrNotExist) && pool.State == "installing" {
				if err = vault.ReplaceFile(ctx, path, want); err != nil {
					return pool, err
				}
			} else {
				return pool, e
			}
		}
		payload := filepath.Join(dir, "Payload.lua")
		actual, e := readSlotFile(payload, 2<<20)
		if errors.Is(e, os.ErrNotExist) && pool.State == "installing" {
			actual = []byte(inertSlot)
			if err = vault.ReplaceFile(ctx, payload, actual); err != nil {
				return pool, err
			}
		} else if e != nil {
			return pool, e
		}
		expected := pool.Files[i-1].PayloadHash
		if expected == "" {
			expected = slotDigest([]byte(inertSlot))
		}
		if slotDigest(actual) != expected {
			return pool, fmt.Errorf("%w: slot payload drift", ErrInstallation)
		}
		pool.Files[i-1].PayloadHash = expected
		if err = exactSlotFileCount(dir); err != nil {
			return pool, err
		}
	}
	pool.State = "ready"
	if err = saveSlotPool(ctx, parent, pool); err != nil {
		return pool, err
	}
	if pool.Schema != slotPoolSchema || pool.Version != version {
		return upgradeSlotPool(ctx, parent, pool, version)
	}
	return pool, nil
}

// All old/new bytes are checked before a resumable per-file replacement. The
// admission lease excludes connections and the runtime refuses mixed versions.
func upgradeSlotPool(ctx context.Context, parent string, pool SlotPool, version string) (SlotPool, error) {
	if pool.State == "ready" {
		if _, err := inspectSlotPool(ctx, parent, pool, pool.Version); err != nil {
			return pool, err
		}
		for _, entry := range pool.Files {
			if entry.PendingHash != "" || entry.Nonce != "" && !entry.Consumed && entry.RetiredRuntime == "" && !entry.RetiredProcess {
				return pool, fmt.Errorf("%w: unresolved slot", ErrConflict)
			}
		}
		// An extension can only adopt paths absent before this durable intent.
		for i := len(pool.Files) + 1; i <= bridge.SlotCount; i++ {
			if _, err := os.Lstat(SlotDirectory(parent, i)); !errors.Is(err, os.ErrNotExist) {
				return pool, fmt.Errorf("%w: unmanaged slot %d", ErrInstallation, i)
			}
		}
		pool.State = "upgrading"
		pool.PendingVersion = version
		pool.SourceSchema = pool.Schema
		pool.Schema = slotPoolMigrationSchema
		if err := saveSlotPool(ctx, parent, pool); err != nil {
			return pool, err
		}
	}
	if pool.State != "upgrading" || pool.PendingVersion != version {
		return pool, ErrConflict
	}
	for _, entry := range pool.Files {
		if entry.PendingHash != "" || entry.Nonce != "" && !entry.Consumed && entry.RetiredRuntime == "" && !entry.RetiredProcess {
			return pool, ErrConflict
		}
	}
	type replacement struct {
		path string
		data []byte
	}
	var writes []replacement
	var directories []string
	// Validate every old, replaced and newly created file before modifying any.
	// The original files stay in the versioned intent until the final commit.
	for i := 1; i <= bridge.SlotCount; i++ {
		if err := ctx.Err(); err != nil {
			return pool, err
		}
		dir := SlotDirectory(parent, i)
		existing := i <= len(pool.Files)
		info, err := os.Lstat(dir)
		absent := errors.Is(err, os.ErrNotExist)
		if err != nil && (!absent || existing) {
			return pool, err
		}
		if !absent {
			if !info.IsDir() {
				return pool, ErrInstallation
			}
			if err = plainSlotDirectory(dir); err != nil {
				return pool, err
			}
		} else {
			directories = append(directories, dir)
		}
		desired := slotStatic(i, version)
		desired["Payload.lua"] = []byte(inertSlot)
		previous := slotStaticForSchema(i, pool.Version, pool.SourceSchema)
		if !absent {
			entries, e := os.ReadDir(dir)
			if e != nil {
				return pool, e
			}
			if existing && len(entries) != 3 {
				return pool, ErrInstallation
			}
			for _, entry := range entries {
				if _, known := desired[entry.Name()]; !known {
					return pool, ErrInstallation
				}
			}
		}
		for name, want := range desired {
			path := filepath.Join(dir, name)
			limit := int64(16384)
			if name == "Payload.lua" {
				limit = 2 << 20
			}
			actual, e := readSlotFile(path, limit)
			if errors.Is(e, os.ErrNotExist) && !existing {
				writes = append(writes, replacement{path, want})
				continue
			}
			if e != nil {
				return pool, e
			}
			hash := slotDigest(actual)
			if hash == slotDigest(want) {
				continue
			}
			allowed := ""
			if existing {
				if name == "Payload.lua" {
					allowed = pool.Files[i-1].PayloadHash
				} else {
					allowed = slotDigest(previous[name])
				}
			}
			legacyTarget := ""
			if existing && pool.Schema == slotPoolMigrationSchema && name != "Payload.lua" {
				legacyTarget = slotDigest(slotStaticForSchema(i, version, pool.SourceSchema)[name])
			}
			if allowed == "" || hash != allowed && hash != legacyTarget {
				return pool, ErrInstallation
			}
			writes = append(writes, replacement{path, want})
		}
	}
	for _, dir := range directories {
		if err := ctx.Err(); err != nil {
			return pool, err
		}
		if err := os.Mkdir(dir, 0700); err != nil {
			return pool, err
		}
		if err := plainSlotDirectory(dir); err != nil {
			return pool, err
		}
	}
	for _, write := range writes {
		if err := vault.ReplaceFile(ctx, write.path, write.data); err != nil {
			return pool, err
		}
	}
	pool.Schema = slotPoolSchema
	pool.Version = version
	pool.PendingVersion = ""
	pool.SourceSchema = ""
	pool.State = "ready"
	pool.Files = make([]SlotFile, bridge.SlotCount)
	for i := range pool.Files {
		pool.Files[i] = SlotFile{PayloadHash: slotDigest([]byte(inertSlot))}
	}
	return pool, saveSlotPool(ctx, parent, pool)
}

// InspectSlots validates static files and the exact currently journaled payloads.
// A pending publication is recognized as pending, never silently marked clean.
func InspectSlots(ctx context.Context, parent, version string) (SlotPool, error) {
	pool, err := readSlotPool(parent)
	if err != nil {
		return pool, err
	}
	if pool.Schema != slotPoolSchema {
		return pool, ErrInstallation
	}
	return inspectSlotPool(ctx, parent, pool, version)
}

// Installation maintenance checks old generated bytes without decoding or
// resuming any old business envelope. Upgrade refuses unresolved reservations.
func inspectSlotPool(ctx context.Context, parent string, pool SlotPool, version string) (SlotPool, error) {
	if pool.State != "ready" || pool.Version != version {
		return pool, ErrInstallation
	}
	var err error
	for i, item := range pool.Files {
		if err = ctx.Err(); err != nil {
			return pool, err
		}
		dir := SlotDirectory(parent, i+1)
		if err = plainSlotDirectory(dir); err != nil {
			return pool, err
		}
		if err = exactSlotFileCount(dir); err != nil {
			return pool, err
		}
		for name, want := range slotStaticForSchema(i+1, version, pool.Schema) {
			actual, e := readSlotFile(filepath.Join(dir, name), 16384)
			if e != nil {
				return pool, e
			}
			if slotDigest(actual) != slotDigest(want) {
				return pool, ErrInstallation
			}
		}
		actual, e := readSlotFile(filepath.Join(dir, "Payload.lua"), 2<<20)
		if e != nil {
			return pool, e
		}
		hash := slotDigest(actual)
		if hash != item.PayloadHash && hash != item.PendingHash {
			return pool, fmt.Errorf("%w: slot payload drift", ErrInstallation)
		}
	}
	return pool, nil
}

// Only the current pool and current envelope can authorize publication.
func slotEnvelopeMatchesPool(pool SlotPool, e bridge.SlotEnvelope) bool {
	return pool.Schema == slotPoolSchema && e.Schema == bridge.SlotSchema
}

// PublishSlot requires the installation publication lease for this disk change.
// Its durable per-slot reservation survives CLI death and protects the payload
// between publication, input and verified receipt reconciliation.
func PublishSlot(ctx context.Context, parent, version, consumer string, envelope bridge.SlotEnvelope) error {
	pool, err := InspectSlots(ctx, parent, version)
	if err != nil {
		return err
	}
	if !slotEnvelopeMatchesPool(pool, envelope) {
		return ErrInstallation
	}
	if envelope.Index < 1 || envelope.Index > len(pool.Files) || consumer == "" {
		return ErrInstallation
	}
	entry := &pool.Files[envelope.Index-1]
	if entry.Nonce != "" && !entry.Consumed && entry.RetiredRuntime == "" && !entry.RetiredProcess && (entry.Nonce != envelope.Nonce || entry.Consumer != consumer || entry.Runtime != envelope.Runtime) {
		return errors.Join(ErrConflict, ErrSlotReserved)
	}
	b, err := bridge.SlotPayload(envelope)
	if err != nil {
		return err
	}
	hash := slotDigest(b)
	if entry.Nonce == envelope.Nonce && ((entry.PendingHash != "" && entry.PendingHash != hash) || (entry.PendingHash == "" && entry.PayloadHash != hash)) {
		return fmt.Errorf("%w: changed slot intent", ErrConflict)
	}
	if entry.Nonce == envelope.Nonce && entry.PayloadHash == hash && entry.PendingHash == "" {
		return nil
	}
	entry.PendingHash = hash
	entry.RetiredRuntime = ""
	entry.RetiredProcess = false
	entry.Nonce = envelope.Nonce
	entry.Runtime = envelope.Runtime
	entry.Consumer = consumer
	entry.Consumed = false
	if err = saveSlotPool(ctx, parent, pool); err != nil {
		return err
	}
	if err = vault.ReplaceFile(ctx, filepath.Join(SlotDirectory(parent, envelope.Index), "Payload.lua"), b); err != nil {
		return err
	}
	entry.PayloadHash = hash
	entry.PendingHash = ""
	return saveSlotPool(ctx, parent, pool)
}

// VerifySlotPublication requires the shared lease through the physical input
// burst. The durable reservation protects the later receipt wait. A resumed
// sender cannot rely on a former OS lock.
func VerifySlotPublication(ctx context.Context, parent, version, consumer string, envelope bridge.SlotEnvelope) error {
	pool, err := InspectSlots(ctx, parent, version)
	if err != nil {
		return err
	}
	if !slotEnvelopeMatchesPool(pool, envelope) {
		return ErrInstallation
	}
	if envelope.Index < 1 || envelope.Index > len(pool.Files) || consumer == "" {
		return ErrInstallation
	}
	e := pool.Files[envelope.Index-1]
	payload, err := bridge.SlotPayload(envelope)
	if err != nil {
		return err
	}
	if e.Nonce != envelope.Nonce || e.Runtime != envelope.Runtime || e.Consumer != consumer || e.PendingHash != "" || e.Consumed || e.RetiredRuntime != "" || e.RetiredProcess || e.PayloadHash != slotDigest(payload) {
		return ErrConflict
	}
	return nil
}

// ConfirmSlotConsumed retires a disk reservation AFTER the caller durably
// verifies its exact runtime receipt. That receipt is the consumption authority.
// A later publication may already occupy the file after a crash between this
// save and the caller's journal advance; leave the newer reservation untouched.
func ConfirmSlotConsumed(ctx context.Context, parent, version, consumer string, index int, nonce string) error {
	pool, err := InspectSlots(ctx, parent, version)
	if err != nil {
		return err
	}
	if index < 1 || index > len(pool.Files) {
		return ErrInstallation
	}
	entry := &pool.Files[index-1]
	if nonce == "" || consumer == "" {
		return ErrConflict
	}
	if entry.Nonce != nonce {
		return nil
	}
	if entry.Consumer != consumer || entry.PendingHash != "" {
		return ErrConflict
	}
	entry.Consumed = true
	return saveSlotPool(ctx, parent, pool)
}
