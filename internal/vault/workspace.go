package vault

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"time"
)

const WorkspaceSchema = "lycheedev.workspace.v1"

var (
	ErrLegacyWorkspace = errors.New("vault.legacy_detected")
	ErrWorkspaceFormat = errors.New("vault.unsupported_workspace")
	// ErrWorkspaceSchemaNewer is joined with ErrWorkspaceFormat so callers that
	// only understand the old error keep working while new callers can report
	// precisely that a newer toolkit wrote this workspace.
	ErrWorkspaceSchemaNewer = errors.New("vault.workspace_schema_newer")
)

type Identity struct {
	Schema      string    `json:"schema"`
	WorkspaceID string    `json:"workspaceId"`
	CreatedAt   time.Time `json:"createdAt"`
}

// Store owns only a new-format workspace. Opening never initializes or migrates.
type Store struct {
	root     string
	identity Identity
}

func (s *Store) Root() string       { return s.root }
func (s *Store) Identity() Identity { return s.identity }

func OpenStore(root string) (*Store, error) {
	abs, err := filepath.Abs(root)
	if err != nil {
		return nil, err
	}
	b, err := os.ReadFile(filepath.Join(abs, "workspace.json"))
	if errors.Is(err, os.ErrNotExist) {
		entries, listErr := os.ReadDir(abs)
		if listErr == nil && len(entries) > 0 {
			return nil, ErrLegacyWorkspace
		}
		return nil, err
	}
	if err != nil {
		return nil, err
	}
	var id Identity
	if json.Unmarshal(b, &id) != nil || len(id.WorkspaceID) != 32 || id.CreatedAt.IsZero() {
		return nil, ErrWorkspaceFormat
	}
	if id.Schema != WorkspaceSchema {
		if workspaceSchemaNewer(id.Schema) {
			return nil, errors.Join(ErrWorkspaceSchemaNewer, ErrWorkspaceFormat)
		}
		return nil, ErrWorkspaceFormat
	}
	if _, err := hex.DecodeString(id.WorkspaceID); err != nil {
		return nil, ErrWorkspaceFormat
	}
	return &Store{root: abs, identity: id}, nil
}

// Initialize publishes a complete directory atomically. Existing data is never
// imported, replaced, or deleted; fresh archival is a separate explicit action.
func Initialize(ctx context.Context, root string) (*Store, error) {
	abs, err := filepath.Abs(root)
	if err != nil {
		return nil, err
	}
	parent := filepath.Dir(abs)
	if abs == parent {
		return nil, fmt.Errorf("vault: workspace cannot be a filesystem root")
	}
	if err := os.MkdirAll(parent, 0700); err != nil {
		return nil, err
	}
	lease, err := AcquireLease(ctx, filepath.Join(parent, ".lycheedev-locks"), "initialize:"+filepath.Base(abs))
	if err != nil {
		return nil, err
	}
	defer lease.Close()
	return initializeLocked(ctx, abs)
}

// initializeLocked performs the staged publish while the caller holds the
// initialize lease for this root, so archive switching can reuse it without
// re-entering the same lease.
func initializeLocked(ctx context.Context, abs string) (*Store, error) {
	parent := filepath.Dir(abs)
	emptyTarget := false
	if info, err := os.Lstat(abs); err == nil {
		if !info.IsDir() {
			return nil, ErrWorkspaceFormat
		}
		entries, listErr := os.ReadDir(abs)
		if listErr != nil {
			return nil, listErr
		}
		if len(entries) > 0 {
			return OpenStore(abs)
		}
		emptyTarget = true
	} else if !errors.Is(err, os.ErrNotExist) {
		return nil, err
	}
	stage, err := os.MkdirTemp(parent, ".lycheedev-init-")
	if err != nil {
		return nil, err
	}
	defer os.RemoveAll(stage) // Only the newly allocated private staging directory.
	for _, name := range []string{"state", "pins", "mirrors", "blobs", "indexes", "cache", "runs", "captures", "locks", "tmp"} {
		if err := os.Mkdir(filepath.Join(stage, name), 0700); err != nil {
			return nil, err
		}
	}
	var random [16]byte
	if _, err := rand.Read(random[:]); err != nil {
		return nil, err
	}
	id := Identity{Schema: WorkspaceSchema, WorkspaceID: hex.EncodeToString(random[:]), CreatedAt: time.Now().UTC()}
	data, err := json.MarshalIndent(id, "", "  ")
	if err != nil {
		return nil, err
	}
	if err := writeDurable(filepath.Join(stage, "workspace.json"), append(data, '\n')); err != nil {
		return nil, err
	}
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	// Remove only the explicitly selected, still-empty directory. Remove fails
	// if another writer added data; never recursively clear an existing target.
	if emptyTarget {
		if err := os.Remove(abs); err != nil {
			return nil, err
		}
	}
	if err := os.Rename(stage, abs); err != nil {
		return nil, err
	}
	return OpenStore(abs)
}

// workspaceSchemaNewer reports whether a marker schema is a future revision of
// the recognized workspace format family.
func workspaceSchemaNewer(schema string) bool {
	const prefix = "lycheedev.workspace.v"
	if !strings.HasPrefix(schema, prefix) {
		return false
	}
	version := 0
	for _, c := range schema[len(prefix):] {
		if c < '0' || c > '9' {
			return false
		}
		version = version*10 + int(c-'0')
	}
	return version > 1
}

func writeDurable(path string, data []byte) error {
	f, err := os.OpenFile(path, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0600)
	if err != nil {
		return err
	}
	_, writeErr := f.Write(data)
	if writeErr == nil {
		writeErr = f.Sync()
	}
	closeErr := f.Close()
	return errors.Join(writeErr, closeErr)
}
