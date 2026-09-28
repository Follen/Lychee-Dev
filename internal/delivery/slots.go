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
	name := fmt.Sprintf("Lychee Dev Slot %02d", index)
	return map[string][]byte{
		name + ".toc": []byte(fmt.Sprintf("## Interface: 120100, 50504, 38002, 16001\n## Title: Lychee Dev input slot %02d\n## Version: %s\n## LoadOnDemand: 1\n## X-Lychee-Slot: %d\n## X-Lychee-Transport: memory-slot-v1\nPayload.lua\nLoader.lua\n", index, version, index)),
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
	if pool.State != "ready" || pool.Version != version {
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
		for name, want := range slotStatic(i+1, version) {
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
	b, err := readSlotFile(filepath.Join(parent, slotMarker), 65536)
	if err != nil {
		return pool, err
	}
	if len(b) > 65536 {
		return pool, ErrInstallation
	}
	if err = json.Unmarshal(b, &pool); err != nil {
		return pool, err
	}
	if pool.Schema != "lycheedev.slots.v1" || len(pool.Files) != bridge.SlotCount || pool.Version == "" {
		return pool, ErrInstallation
	}
	return pool, nil
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
		pool = SlotPool{Schema: "lycheedev.slots.v1", Version: version, State: "installing", Files: make([]SlotFile, bridge.SlotCount)}
		if err = saveSlotPool(ctx, parent, pool); err != nil {
			return pool, err
		}
	} else if err != nil {
		return pool, err
	} else if pool.Version != version {
		return upgradeSlotPool(ctx, parent, pool, version)
	}
	for i := 1; i <= bridge.SlotCount; i++ {
		dir := SlotDirectory(parent, i)
		if err = os.MkdirAll(dir, 0700); err != nil {
			return pool, err
		}
		if err = plainSlotDirectory(dir); err != nil {
			return pool, err
		}
		for name, want := range slotStatic(i, version) {
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
	return pool, saveSlotPool(ctx, parent, pool)
}

// All old/new bytes are checked before a resumable per-file replacement. The
// admission lease excludes connections and the runtime refuses mixed versions.
func upgradeSlotPool(ctx context.Context, parent string, pool SlotPool, version string) (SlotPool, error) {
	if pool.State == "ready" {
		if _, err := InspectSlots(ctx, parent, pool.Version); err != nil {
			return pool, err
		}
		for _, entry := range pool.Files {
			if entry.PendingHash != "" || (entry.Nonce != "" && !entry.Consumed && entry.RetiredRuntime == "" && !entry.RetiredProcess) {
				return pool, fmt.Errorf("%w: unresolved slot", ErrConflict)
			}
		}
		pool.State = "upgrading"
		pool.PendingVersion = version
		if err := saveSlotPool(ctx, parent, pool); err != nil {
			return pool, err
		}
	}
	if pool.State != "upgrading" || pool.PendingVersion != version {
		return pool, ErrConflict
	}
	for i := 1; i <= bridge.SlotCount; i++ {
		dir := SlotDirectory(parent, i)
		if err := plainSlotDirectory(dir); err != nil {
			return pool, err
		}
		old := slotStatic(i, pool.Version)
		for name, want := range slotStatic(i, version) {
			path := filepath.Join(dir, name)
			actual, err := readSlotFile(path, 16384)
			if err != nil {
				return pool, err
			}
			if slotDigest(actual) != slotDigest(want) && slotDigest(actual) != slotDigest(old[name]) {
				return pool, ErrInstallation
			}
			if err = vault.ReplaceFile(ctx, path, want); err != nil {
				return pool, err
			}
		}
		path := filepath.Join(dir, "Payload.lua")
		actual, err := readSlotFile(path, 2<<20)
		if err != nil {
			return pool, err
		}
		if slotDigest(actual) != pool.Files[i-1].PayloadHash && slotDigest(actual) != slotDigest([]byte(inertSlot)) {
			return pool, ErrInstallation
		}
		if err = vault.ReplaceFile(ctx, path, []byte(inertSlot)); err != nil {
			return pool, err
		}
	}
	pool.Version = version
	pool.PendingVersion = ""
	pool.State = "ready"
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
	if pool.State != "ready" || pool.Version != version {
		return pool, ErrInstallation
	}
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
		for name, want := range slotStatic(i+1, version) {
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

// PublishSlot requires the installation publication lease for this disk change.
// Its durable per-slot reservation survives CLI death and protects the payload
// between publication, input and verified receipt reconciliation.
func PublishSlot(ctx context.Context, parent, version, consumer string, envelope bridge.SlotEnvelope) error {
	pool, err := InspectSlots(ctx, parent, version)
	if err != nil {
		return err
	}
	if envelope.Index < 1 || envelope.Index > bridge.SlotCount || consumer == "" {
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
	if envelope.Index < 1 || envelope.Index > bridge.SlotCount || consumer == "" {
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
	if index < 1 || index > bridge.SlotCount {
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
