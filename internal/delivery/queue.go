package delivery

import (
	"bytes"
	"context"
	"crypto/rand"
	"crypto/sha256"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/vault"
)

var ErrQueueConflict = errors.New("delivery.queue_request_conflict")

type QueueRevision struct {
	SHA256  string `json:"sha256"`
	Entries int    `json:"entries"`
	Changed bool   `json:"changed"`
}

// ChangeProbeQueue merges into an existing canonical queue under the same
// parent-scoped lease used by installation/upgrade/removal. It does not install,
// reload, execute, acknowledge or claim the client has loaded this revision.
// retire removes only a byte-identical definition; retries are idempotent.
// Ownership and immutable content are checked under the lease. Callers must
// still record operation intent before this external effect.
func ChangeProbeQueue(ctx context.Context, addonDirectory string, definition bridge.ProbeDefinition, retire bool) (revision QueueRevision, err error) {
	return changeProbeQueue(ctx, addonDirectory, definition, retire, false)
}

// VerifyProbeRetired checks the managed loaded-file source under its installation
// lease without removing a reappeared entry or rewriting any queue bytes.
func VerifyProbeRetired(ctx context.Context, addonDirectory string, definition bridge.ProbeDefinition) (QueueRevision, error) {
	return changeProbeQueue(ctx, addonDirectory, definition, true, true)
}

// VerifyProbePrepared checks exact intent without publishing a missing entry.
func VerifyProbePrepared(ctx context.Context, addonDirectory string, definition bridge.ProbeDefinition) (QueueRevision, error) {
	return changeProbeQueue(ctx, addonDirectory, definition, false, true)
}

func changeProbeQueue(ctx context.Context, addonDirectory string, definition bridge.ProbeDefinition, retire bool, verifyOnly bool) (revision QueueRevision, err error) {
	if err = ctx.Err(); err != nil {
		return revision, err
	}
	if _, err = bridge.EncodeProbeQueue([]bridge.ProbeDefinition{definition}); err != nil {
		return revision, err
	}
	abs, err := filepath.Abs(addonDirectory)
	if err != nil {
		return revision, err
	}
	parent, err := filepath.EvalSymlinks(filepath.Dir(abs))
	if err != nil {
		return revision, err
	}
	target := filepath.Join(parent, filepath.Base(abs))
	if err = ordinaryQueuePath(target, true); err != nil {
		return revision, err
	}
	scope := filepath.Join(parent, ".lycheedev-locks")
	if err = os.Mkdir(scope, 0700); err != nil && !errors.Is(err, os.ErrExist) {
		return revision, err
	}
	if err = ordinaryQueuePath(scope, true); err != nil {
		return revision, err
	}
	resource := "installation:" + strings.ToLower(target)
	lockDigest := sha256.Sum256([]byte(resource))
	lockPath := filepath.Join(scope, fmt.Sprintf("%x.lock", lockDigest))
	if err = ordinaryQueuePath(lockPath, false); err != nil && !errors.Is(err, os.ErrNotExist) {
		return revision, err
	}
	lease, err := vault.AcquireLease(ctx, scope, resource)
	if err != nil {
		return revision, err
	}
	defer func() { err = errors.Join(err, lease.Close()) }()
	if err = ordinaryQueuePath(target, true); err != nil {
		return revision, err
	}
	if err = ordinaryQueuePath(filepath.Join(target, "Bridge"), true); err != nil {
		return revision, err
	}
	assessment, err := inspectInstallation(ctx, target, "addon", true)
	if err != nil {
		return revision, err
	}
	if assessment.State != "managed" || assessment.Receipt.Version != definition.Release {
		return revision, fmt.Errorf("%w: queue requires matching managed addon", ErrInstallation)
	}
	root, err := os.OpenRoot(filepath.Join(target, "Bridge"))
	if err != nil {
		return revision, err
	}
	defer root.Close()
	read := func() ([]byte, error) {
		info, err := root.Lstat("Definitions.lua")
		if err != nil {
			return nil, err
		}
		if !info.Mode().IsRegular() || info.Size() > bridge.MaxProbeQueueFileBytes {
			return nil, errors.New("delivery.queue_invalid_file")
		}
		file, err := root.Open("Definitions.lua")
		if err != nil {
			return nil, err
		}
		data, readErr := io.ReadAll(io.LimitReader(file, bridge.MaxProbeQueueFileBytes+1))
		closeErr := file.Close()
		return data, errors.Join(readErr, closeErr)
	}
	before, err := read()
	if err != nil {
		return revision, err
	}
	definitions, err := bridge.DecodeProbeQueue(bytes.NewReader(before))
	if err != nil {
		return revision, err
	}
	found := false
	for i, d := range definitions {
		if d.RequestID != definition.RequestID {
			continue
		}
		if verifyOnly && retire {
			return revision, ErrQueueConflict
		}
		if d != definition {
			return revision, ErrQueueConflict
		}
		found = true
		if retire {
			definitions = append(definitions[:i], definitions[i+1:]...)
		}
		break
	}
	if verifyOnly {
		if !retire && !found {
			return revision, ErrQueueConflict
		}
		return QueueRevision{SHA256: fmt.Sprintf("%x", sha256.Sum256(before)), Entries: len(definitions)}, ctx.Err()
	}
	if !retire && !found {
		definitions = append(definitions, definition)
	}
	after, err := bridge.EncodeProbeQueue(definitions)
	if err != nil {
		return revision, err
	}
	revision = QueueRevision{SHA256: fmt.Sprintf("%x", sha256.Sum256(after)), Entries: len(definitions), Changed: !bytes.Equal(before, after)}
	if err = ctx.Err(); err != nil {
		return QueueRevision{}, err
	}
	if !revision.Changed {
		return revision, nil
	}
	var nonce [16]byte
	if _, err = rand.Read(nonce[:]); err != nil {
		return QueueRevision{}, err
	}
	temporary := fmt.Sprintf(".lycheedev-queue-%x.tmp", nonce)
	file, err := root.OpenFile(temporary, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
	if err != nil {
		return QueueRevision{}, err
	}
	defer func() {
		removeErr := root.Remove(temporary)
		if !errors.Is(removeErr, os.ErrNotExist) {
			err = errors.Join(err, removeErr)
		}
	}()
	_, writeErr := file.Write(after)
	syncErr := file.Sync()
	closeErr := file.Close()
	if err = errors.Join(writeErr, syncErr, closeErr); err != nil {
		return QueueRevision{}, err
	}
	current, err := read()
	if err != nil {
		return QueueRevision{}, err
	}
	if !bytes.Equal(before, current) {
		return QueueRevision{}, ErrQueueConflict
	}
	if err = ctx.Err(); err != nil {
		return QueueRevision{}, err
	}
	// Publish with one same-directory replacement, never truncate the live Lua
	// file. The lease serializes cooperating processes, not arbitrary editors.
	// This is not a power-loss durability or hostile-filesystem guarantee.
	if err = root.Rename(temporary, "Definitions.lua"); err != nil {
		return QueueRevision{}, err
	}
	return revision, nil
}

func ordinaryQueuePath(path string, directory bool) error {
	info, err := os.Lstat(path)
	if err != nil {
		return err
	}
	if info.Mode()&os.ModeSymlink != 0 || (directory && !info.IsDir()) || (!directory && !info.Mode().IsRegular()) {
		return errors.New("delivery.queue_redirected_path")
	}
	resolved, err := filepath.EvalSymlinks(path)
	if err != nil {
		return err
	}
	if !strings.EqualFold(filepath.Clean(path), filepath.Clean(resolved)) {
		return errors.New("delivery.queue_redirected_path")
	}
	return nil
}
