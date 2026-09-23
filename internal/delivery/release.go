package delivery

import (
	"bytes"
	"context"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
	"unicode/utf8"
)

var ErrRelease = errors.New("delivery.invalid_release")

// SourceArchive is the corresponding-source record required by the combined-work
// licensing terms: the exact repository state that builds this release, shipped
// as a self-contained source package. The npm assembly gate requires it; a
// single-platform native archive may omit it.
type SourceArchive struct {
	Repository string `json:"repository"`
	Commit     string `json:"commit"`
	Tag        string `json:"tag"`
	Archive    string `json:"archive"`
	SHA256     string `json:"sha256"`
}

type Release struct {
	Schema              string              `json:"schema"`
	Version             string              `json:"version"`
	Commit              string              `json:"commit"`
	Binaries            map[string]Resource `json:"binaries"`
	Resources           []Resource          `json:"resources"`
	CorrespondingSource *SourceArchive      `json:"correspondingSource,omitempty"`
}

// validateSourceArchive checks the optional corresponding-source record. The
// recorded commit is the release's own binding key: it must equal the release
// commit so the tag target, the in-binary VCS stamp and the source archive all
// describe one tree.
func validateSourceArchive(source *SourceArchive, releaseCommit string) error {
	if source == nil {
		return nil
	}
	commit, commitErr := hex.DecodeString(source.Commit)
	digest, digestErr := hex.DecodeString(source.SHA256)
	if !utf8.ValidString(source.Repository) || source.Repository == "" || len(source.Repository) > 512 ||
		commitErr != nil || len(commit) != 20 || strings.ToLower(source.Commit) != source.Commit ||
		source.Commit != releaseCommit ||
		source.Tag == "" || len(source.Tag) > 128 || strings.ContainsAny(source.Tag, "\r\n\t") ||
		source.Archive == "" || len(source.Archive) > 255 || strings.ContainsAny(source.Archive, `/\`) ||
		digestErr != nil || len(digest) != 32 || strings.ToLower(source.SHA256) != source.SHA256 {
		return fmt.Errorf("%w: corresponding source", ErrRelease)
	}
	return nil
}

// InspectRelease reads the common npm/native release manifest and verifies the
// deployable payload. Binary records are validated here, but binary bytes remain
// the launcher's responsibility. The release declares the shipped platform set
// (windows-amd64 only for 2.0); the npm assembly gate must require exactly it.
// This does not authenticate a release.
func InspectRelease(ctx context.Context, directory, expectedVersion string) (Release, error) {
	var release Release
	root, err := os.OpenRoot(directory)
	if err != nil {
		return release, err
	}
	defer root.Close()
	info, err := root.Lstat("release.json")
	if err != nil {
		return release, err
	}
	if !info.Mode().IsRegular() || info.Size() > 1<<20 {
		return release, fmt.Errorf("%w: manifest file", ErrRelease)
	}
	file, err := root.Open("release.json")
	if err != nil {
		return release, err
	}
	defer file.Close()
	raw, err := io.ReadAll(&cancelReader{ctx: ctx, reader: io.LimitReader(file, (1<<20)+1)})
	if err != nil {
		return release, err
	}
	if len(raw) > 1<<20 || !utf8.Valid(raw) {
		return release, fmt.Errorf("%w: manifest encoding or size", ErrRelease)
	}
	decoder := json.NewDecoder(bytes.NewReader(raw))
	if err = uniqueJSON(decoder, 0); err != nil {
		return release, fmt.Errorf("%w: %v", ErrRelease, err)
	}
	if _, err = decoder.Token(); err != io.EOF {
		return release, fmt.Errorf("%w: trailing JSON", ErrRelease)
	}
	decoder = json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	if err = decoder.Decode(&release); err != nil {
		return Release{}, fmt.Errorf("%w: %v", ErrRelease, err)
	}
	commit, err := hex.DecodeString(release.Commit)
	if release.Schema != "lycheedev.release.v1" || expectedVersion == "" || release.Version != expectedVersion || err != nil || len(commit) != 20 || strings.ToLower(release.Commit) != release.Commit {
		return Release{}, fmt.Errorf("%w: schema, version or commit", ErrRelease)
	}
	if err = validateSourceArchive(release.CorrespondingSource, release.Commit); err != nil {
		return Release{}, err
	}
	if len(release.Binaries) == 0 || len(release.Binaries) > 5 {
		return Release{}, fmt.Errorf("%w: binaries", ErrRelease)
	}
	for target, binary := range release.Binaries {
		name := "lycheedev"
		switch target {
		case "windows-amd64":
			name += ".exe"
		case "linux-amd64", "linux-arm64", "darwin-amd64", "darwin-arm64":
		default:
			return Release{}, fmt.Errorf("%w: platform %q", ErrRelease, target)
		}
		digest, decodeErr := hex.DecodeString(binary.SHA256)
		if binary.Path != "native/"+target+"/"+name || binary.Bytes < 1 || binary.Bytes > maxPayloadBytes || decodeErr != nil || len(digest) != 32 || strings.ToLower(binary.SHA256) != binary.SHA256 {
			return Release{}, fmt.Errorf("%w: binary record %q", ErrRelease, target)
		}
	}
	required := map[string]bool{
		"skill/SKILL.md":                false,
		"addon/Lychee Dev_Mainline.toc": false,
		"addon/Lychee Dev_Mists.toc":    false,
		"addon/Lychee Dev_Wrath.toc":    false,
		"addon/Lychee Dev_Forever.toc":  false,
	}
	for _, resource := range release.Resources {
		if _, ok := required[resource.Path]; ok {
			required[resource.Path] = true
		}
	}
	for name, present := range required {
		if !present {
			return Release{}, fmt.Errorf("%w: required resource %q", ErrRelease, name)
		}
	}
	info, err = root.Lstat("payload")
	if err != nil {
		return Release{}, err
	}
	if !info.IsDir() || info.Mode()&os.ModeSymlink != 0 {
		return Release{}, fmt.Errorf("%w: payload directory", ErrRelease)
	}
	if err = VerifyPayload(ctx, filepath.Join(directory, "payload"), release.Resources); err != nil {
		return Release{}, err
	}
	return release, nil
}

// encoding/json otherwise silently accepts duplicate keys, including escaped
// spellings of the same key. Bound nesting before decoding the typed manifest.
func uniqueJSON(decoder *json.Decoder, depth int) error {
	if depth > 16 {
		return errors.New("JSON nesting limit")
	}
	token, err := decoder.Token()
	if err != nil {
		return err
	}
	delimiter, container := token.(json.Delim)
	if !container {
		return nil
	}
	switch delimiter {
	case '{':
		seen := make(map[string]bool)
		for decoder.More() {
			key, err := decoder.Token()
			if err != nil {
				return err
			}
			name, ok := key.(string)
			// The typed decoder matches struct fields case-insensitively.
			// Reject aliases that would otherwise overwrite the same field.
			name = strings.ToLower(name)
			if !ok || seen[name] {
				return errors.New("duplicate or invalid JSON key")
			}
			seen[name] = true
			if err := uniqueJSON(decoder, depth+1); err != nil {
				return err
			}
		}
	case '[':
		for decoder.More() {
			if err := uniqueJSON(decoder, depth+1); err != nil {
				return err
			}
		}
	default:
		return errors.New("invalid JSON delimiter")
	}
	_, err = decoder.Token()
	return err
}
