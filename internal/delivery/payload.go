// Package delivery verifies and deploys versioned toolkit resources. It never
// discovers or imports legacy installations as part of release verification.
package delivery

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"io"
	"io/fs"
	"os"
	"path"
	"strings"
)

var ErrPayload = errors.New("delivery.invalid_payload")

// Resource paths are relative to the payload directory, using slash separators.
// Only addon/ and skill/ are deployable; the npm wrapper is not a resource.
type Resource struct {
	Path   string `json:"path"`
	Bytes  int64  `json:"bytes"`
	SHA256 string `json:"sha256"`
}

const maxPayloadBytes int64 = 256 << 20

// VerifyPayload checks the entire inventory before deployment can start. The
// source must remain immutable until deployment; this is not an authentication
// mechanism for an untrusted manifest. Release authenticity is checked upstream.
// Links, unlisted files and case-insensitive collisions are rejected on every OS
// so the same payload has one meaning on Windows and Unix.
func VerifyPayload(ctx context.Context, directory string, inventory []Resource) error {
	return verifyInventory(ctx, directory, inventory, "")
}

// component is nonempty only for an installed tree with a checked receipt.
func verifyInventory(ctx context.Context, directory string, inventory []Resource, component string) error {
	if len(inventory) == 0 || len(inventory) > 32768 {
		return fmt.Errorf("%w: inventory size", ErrPayload)
	}
	entries := make(map[string]Resource, len(inventory))
	var total int64
	for _, entry := range inventory {
		if !resourcePath(entry.Path) || entry.Bytes < 0 || entry.Bytes > maxPayloadBytes-total {
			return fmt.Errorf("%w: resource %q", ErrPayload, entry.Path)
		}
		digest, err := hex.DecodeString(entry.SHA256)
		if err != nil || len(digest) != sha256.Size || strings.ToLower(entry.SHA256) != entry.SHA256 {
			return fmt.Errorf("%w: digest %q", ErrPayload, entry.Path)
		}
		key := strings.ToLower(entry.Path)
		if component != "" {
			if !strings.HasPrefix(entry.Path, component+"/") {
				return fmt.Errorf("%w: component mismatch", ErrPayload)
			}
			entry.Path = strings.TrimPrefix(entry.Path, component+"/")
			key = strings.ToLower(entry.Path)
			if key == installationMarker {
				return fmt.Errorf("%w: reserved installation marker", ErrPayload)
			}
		}
		if _, exists := entries[key]; exists {
			return fmt.Errorf("%w: duplicate %q", ErrPayload, entry.Path)
		}
		entries[key] = entry
		total += entry.Bytes
	}
	for key := range entries {
		for parent := path.Dir(key); parent != "."; parent = path.Dir(parent) {
			if _, exists := entries[parent]; exists {
				return fmt.Errorf("%w: file-directory collision %q", ErrPayload, parent)
			}
		}
	}
	root, err := os.OpenRoot(directory)
	if err != nil {
		return err
	}
	defer root.Close()
	seen := make(map[string]bool, len(entries))
	paths := make(map[string]bool)
	nodes := 0
	err = fs.WalkDir(root.FS(), ".", func(name string, item fs.DirEntry, walkErr error) error {
		if err := ctx.Err(); err != nil {
			return err
		}
		if walkErr != nil {
			return walkErr
		}
		nodes++
		if nodes > 65536 {
			return fmt.Errorf("%w: directory budget", ErrPayload)
		}
		if name == "." {
			return nil
		}
		if component != "" && name == installationMarker {
			if !item.Type().IsRegular() {
				return fmt.Errorf("%w: installation marker", ErrPayload)
			}
			return nil
		}
		key := strings.ToLower(name)
		if paths[key] || item.Type()&os.ModeSymlink != 0 {
			return fmt.Errorf("%w: link or path collision %q", ErrPayload, name)
		}
		paths[key] = true
		if item.IsDir() {
			logical := name
			if component != "" {
				logical = component + "/" + name
			}
			if !resourcePath(logical + "/placeholder") {
				return fmt.Errorf("%w: directory %q", ErrPayload, name)
			}
			return nil
		}
		entry, exists := entries[key]
		if !exists || entry.Path != name || !item.Type().IsRegular() {
			return fmt.Errorf("%w: unlisted or special file %q", ErrPayload, name)
		}
		file, err := root.Open(name)
		if err != nil {
			return err
		}
		defer file.Close()
		info, err := file.Stat()
		if err != nil {
			return err
		}
		if !info.Mode().IsRegular() || info.Size() != entry.Bytes {
			return fmt.Errorf("%w: size %q", ErrPayload, name)
		}
		hash := sha256.New()
		reader := &cancelReader{ctx: ctx, reader: io.LimitReader(file, entry.Bytes+1)}
		n, err := io.Copy(hash, reader)
		if err != nil {
			return err
		}
		if n != entry.Bytes || hex.EncodeToString(hash.Sum(nil)) != entry.SHA256 {
			return fmt.Errorf("%w: content %q", ErrPayload, name)
		}
		seen[key] = true
		return nil
	})
	if err != nil {
		return err
	}
	if len(seen) != len(entries) {
		return fmt.Errorf("%w: missing resources", ErrPayload)
	}
	return nil
}

func resourcePath(name string) bool {
	if !fs.ValidPath(name) || len(name) > 240 || (!strings.HasPrefix(name, "addon/") && !strings.HasPrefix(name, "skill/")) {
		return false
	}
	parts := strings.Split(name, "/")
	if strings.EqualFold(parts[1], installationMarker) {
		return false
	}
	for _, part := range parts {
		if strings.TrimRight(part, " .") != part || strings.ContainsAny(part, `\:<>"|?*`) {
			return false
		}
		for _, char := range part {
			if char < 32 || char == 127 {
				return false
			}
		}
		base := strings.ToUpper(strings.SplitN(part, ".", 2)[0])
		if base == "CON" || base == "PRN" || base == "AUX" || base == "NUL" || (len([]rune(base)) == 4 && (strings.HasPrefix(base, "COM") || strings.HasPrefix(base, "LPT")) && strings.ContainsRune("123456789¹²³", []rune(base)[3])) {
			return false
		}
	}
	return true
}

type cancelReader struct {
	ctx    context.Context
	reader io.Reader
}

func (r *cancelReader) Read(p []byte) (int, error) {
	if err := r.ctx.Err(); err != nil {
		return 0, err
	}
	return r.reader.Read(p)
}
