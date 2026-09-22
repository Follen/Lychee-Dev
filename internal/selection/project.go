package selection

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
	"unicode/utf8"

	"github.com/follenfang/lycheedev/internal/vault"
)

var (
	ErrProjectMissing  = errors.New("selection.project_missing")
	ErrProjectFormat   = errors.New("selection.project_format")
	ErrProjectUnlocked = errors.New("selection.project_unlocked")
	ErrProjectConflict = errors.New("selection.project_conflict")
)

const projectFile = "lycheedev.json"
const projectLockFile = "lycheedev.lock.json"

type Project struct {
	Schema  string `json:"schema"`
	Product string `json:"product"`
}

type ProjectLock struct {
	Schema    string    `json:"schema"`
	Selection PinnedSet `json:"selection"`
}

type ProjectStatus struct {
	Directory string       `json:"directory"`
	Project   Project      `json:"project"`
	State     string       `json:"state"`
	Lock      *ProjectLock `json:"lock,omitempty"`
}

// FindProject selects the nearest declaration, never a global last-used target.
// A malformed nearer declaration is returned for validation, not skipped.
func FindProject(start string) (string, error) {
	dir, err := filepath.Abs(start)
	if err != nil {
		return "", err
	}
	for {
		_, err := os.Lstat(filepath.Join(dir, projectFile))
		if err == nil {
			return dir, nil
		}
		if !errors.Is(err, os.ErrNotExist) {
			return "", err
		}
		parent := filepath.Dir(dir)
		if parent == dir {
			return "", ErrProjectMissing
		}
		dir = parent
	}
}

// InitializeProject creates only the declaration. It neither selects latest nor
// writes a lock; interrupted setup remains an ordinary unlocked project.
func InitializeProject(ctx context.Context, directory, product string) (ProjectStatus, error) {
	if !projectProduct(product) {
		return ProjectStatus{}, ErrProjectFormat
	}
	dir, err := projectDirectory(directory)
	if err != nil {
		return ProjectStatus{}, err
	}
	lease, err := vault.AcquireLease(ctx, filepath.Join(dir, ".lycheedev-locks"), "project")
	if err != nil {
		return ProjectStatus{}, err
	}
	defer lease.Close()
	if _, err := os.Lstat(filepath.Join(dir, projectFile)); err == nil {
		status, err := InspectProject(ctx, dir)
		if err != nil {
			return ProjectStatus{}, err
		}
		if status.Project.Product != product {
			return ProjectStatus{}, ErrProjectConflict
		}
		return status, nil
	} else if !errors.Is(err, os.ErrNotExist) {
		return ProjectStatus{}, err
	}
	// Do not bless an orphan lock left by another project or checkout.
	if _, err := os.Lstat(filepath.Join(dir, projectLockFile)); !errors.Is(err, os.ErrNotExist) {
		if err != nil {
			return ProjectStatus{}, err
		}
		return ProjectStatus{}, ErrProjectConflict
	}
	project := Project{Schema: "lycheedev.project.v1", Product: product}
	if err := writeProjectFile(ctx, dir, projectFile, project); err != nil {
		return ProjectStatus{}, err
	}
	return ProjectStatus{Directory: dir, Project: project, State: "unlocked"}, nil
}

// LockProject selects an already resolved snapshot. Network and version
// resolution remain in their existing owners, not a second project resolver.
func LockProject(ctx context.Context, workspace, directory, snapshot string) (ProjectStatus, error) {
	pin, err := InspectSelection(ctx, workspace, snapshot)
	if err != nil {
		return ProjectStatus{}, err
	}
	dir, err := projectDirectory(directory)
	if err != nil {
		return ProjectStatus{}, err
	}
	lease, err := vault.AcquireLease(ctx, filepath.Join(dir, ".lycheedev-locks"), "project")
	if err != nil {
		return ProjectStatus{}, err
	}
	defer lease.Close()
	project, err := readProject(ctx, dir)
	if err != nil {
		return ProjectStatus{}, err
	}
	if pinProduct(pin) != project.Product {
		return ProjectStatus{}, ErrProjectConflict
	}
	lock := ProjectLock{Schema: "lycheedev.project-lock.v1", Selection: pin}
	// Existing malformed or foreign-format locks require deliberate repair,
	// rather than being overwritten as a side effect of selecting a snapshot.
	var previous ProjectLock
	if err := readProjectFile(ctx, filepath.Join(dir, projectLockFile), &previous); err == nil {
		if previous.Schema != "lycheedev.project-lock.v1" {
			return ProjectStatus{}, ErrProjectFormat
		}
		if err := validatePinnedSet(previous.Selection); err != nil {
			return ProjectStatus{}, fmt.Errorf("%w: %v", ErrProjectFormat, err)
		}
	} else if !errors.Is(err, os.ErrNotExist) {
		return ProjectStatus{}, err
	}
	if err := writeProjectFile(ctx, dir, projectLockFile, lock); err != nil {
		return ProjectStatus{}, err
	}
	return ProjectStatus{Directory: dir, Project: project, State: "locked", Lock: &lock}, nil
}

// InspectProject is independent of a workspace and never creates files.
func InspectProject(ctx context.Context, directory string) (ProjectStatus, error) {
	dir, err := projectDirectory(directory)
	if err != nil {
		return ProjectStatus{}, err
	}
	project, err := readProject(ctx, dir)
	if err != nil {
		return ProjectStatus{}, err
	}
	status := ProjectStatus{Directory: dir, Project: project, State: "unlocked"}
	var lock ProjectLock
	if err := readProjectFile(ctx, filepath.Join(dir, projectLockFile), &lock); errors.Is(err, os.ErrNotExist) {
		return status, nil
	} else if err != nil {
		return ProjectStatus{}, err
	}
	if lock.Schema != "lycheedev.project-lock.v1" {
		return ProjectStatus{}, ErrProjectFormat
	}
	if err := validatePinnedSet(lock.Selection); err != nil {
		return ProjectStatus{}, fmt.Errorf("%w: %v", ErrProjectFormat, err)
	}
	if pinProduct(lock.Selection) != project.Product {
		return ProjectStatus{}, ErrProjectConflict
	}
	status.State, status.Lock = "locked", &lock
	return status, nil
}

// LoadProjectSelection imports only the complete, hash-checked fixed reference.
// It does not import another workspace's content, sessions, account or settings.
func LoadProjectSelection(ctx context.Context, workspace, directory string) (ProjectStatus, error) {
	status, err := InspectProject(ctx, directory)
	if err != nil {
		return ProjectStatus{}, err
	}
	if status.Lock == nil {
		return ProjectStatus{}, ErrProjectUnlocked
	}
	pin := status.Lock.Selection
	_, err = vault.WriteMetadata(ctx, workspace, func(_ *vault.Store, m *vault.Metadata) (PinnedSet, error) {
		raw, err := json.Marshal(pin)
		if err != nil {
			return PinnedSet{}, err
		}
		err = m.CommitDocuments(ctx, vault.Mutation{Key: "pin/" + pin.ID, Value: raw})
		if err != nil && !errors.Is(err, vault.ErrGeneration) {
			return PinnedSet{}, err
		}
		return OpenPinner(m).ReadPinnedSet(ctx, pin.ID)
	})
	return status, err
}

func pinProduct(pin PinnedSet) string {
	if pin.Source != nil {
		return pin.Source.Product
	}
	if pin.Data != nil {
		return pin.Data.Product
	}
	if pin.Changes != nil {
		return pin.Changes.Product
	}
	return ""
}

func projectProduct(s string) bool {
	if s == "" || len(s) > 64 {
		return false
	}
	for _, r := range s {
		if !(r >= 'a' && r <= 'z' || r >= '0' && r <= '9' || r == '-' || r == '_') {
			return false
		}
	}
	return true
}

func projectDirectory(path string) (string, error) {
	if path == "" {
		path = "."
	}
	dir, err := filepath.Abs(path)
	if err != nil {
		return "", err
	}
	info, err := os.Stat(dir)
	if err != nil {
		return "", err
	}
	if !info.IsDir() {
		return "", ErrProjectFormat
	}
	return dir, nil
}

func readProject(ctx context.Context, dir string) (Project, error) {
	var project Project
	err := readProjectFile(ctx, filepath.Join(dir, projectFile), &project)
	if errors.Is(err, os.ErrNotExist) {
		return project, ErrProjectMissing
	}
	if err != nil {
		return project, err
	}
	if project.Schema != "lycheedev.project.v1" || !projectProduct(project.Product) {
		return project, ErrProjectFormat
	}
	return project, nil
}

func readProjectFile(ctx context.Context, path string, into any) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	info, err := os.Lstat(path)
	if err != nil {
		return err
	}
	if !info.Mode().IsRegular() || info.Size() > 65536 {
		return ErrProjectFormat
	}
	f, err := openProjectFile(path)
	if err != nil {
		return err
	}
	defer f.Close()
	raw, err := io.ReadAll(io.LimitReader(f, 65537))
	if err != nil {
		return err
	}
	if len(raw) > 65536 || !utf8.Valid(raw) {
		return ErrProjectFormat
	}
	// Reject duplicate members at every nesting level before typed decoding.
	d := json.NewDecoder(bytes.NewReader(raw))
	if err := projectJSONValue(d, 0); err != nil {
		return err
	}
	if _, err := d.Token(); err != io.EOF {
		return ErrProjectFormat
	}
	d = json.NewDecoder(bytes.NewReader(raw))
	d.DisallowUnknownFields()
	if err := d.Decode(into); err != nil {
		return fmt.Errorf("%w: %v", ErrProjectFormat, err)
	}
	return ctx.Err()
}

func openProjectFile(path string) (*os.File, error) {
	root, err := os.OpenRoot(filepath.Dir(path))
	if err != nil {
		return nil, err
	}
	defer root.Close()
	// Root.Open allows delete sharing on Windows. The returned file owns its
	// handle and remains valid across a Root.Rename replacement of the lock.
	return root.Open(filepath.Base(path))
}

func projectJSONValue(d *json.Decoder, depth int) error {
	if depth > 16 {
		return ErrProjectFormat
	}
	token, err := d.Token()
	if err != nil {
		return ErrProjectFormat
	}
	delim, ok := token.(json.Delim)
	if !ok {
		return nil
	}
	if delim != '{' && delim != '[' {
		return ErrProjectFormat
	}
	seen := map[string]bool{}
	for d.More() {
		if delim == '{' {
			key, err := d.Token()
			if err != nil {
				return ErrProjectFormat
			}
			name, ok := key.(string)
			name = strings.ToLower(name)
			if !ok || seen[name] {
				return ErrProjectFormat
			}
			seen[name] = true
		}
		if err := projectJSONValue(d, depth+1); err != nil {
			return err
		}
	}
	end, err := d.Token()
	if err != nil || delim == '{' && end != json.Delim('}') || delim == '[' && end != json.Delim(']') {
		return ErrProjectFormat
	}
	return nil
}

func writeProjectFile(ctx context.Context, dir, name string, value any) error {
	target := filepath.Join(dir, name)
	if info, err := os.Lstat(target); err == nil && !info.Mode().IsRegular() {
		return ErrProjectFormat
	} else if err != nil && !errors.Is(err, os.ErrNotExist) {
		return err
	}
	raw, err := json.MarshalIndent(value, "", "  ")
	if err != nil {
		return err
	}
	if len(raw) > 65535 {
		return ErrProjectFormat
	}
	f, err := os.CreateTemp(dir, ".lycheedev-project-")
	if err != nil {
		return err
	}
	defer os.Remove(f.Name())
	_, err = f.Write(append(raw, '\n'))
	if err == nil {
		err = f.Sync()
	}
	err = errors.Join(err, f.Close())
	if err != nil {
		return err
	}
	if err := ctx.Err(); err != nil {
		return err
	}
	root, err := os.OpenRoot(dir)
	if err != nil {
		return err
	}
	defer root.Close()
	if name == projectFile {
		// Creation must not replace a file that appeared after validation.
		return root.Link(filepath.Base(f.Name()), name)
	}
	// Root.Rename uses Windows POSIX replacement semantics: existing readers
	// retain the old file, and subsequent readers open the new complete file.
	return root.Rename(filepath.Base(f.Name()), name)
}
