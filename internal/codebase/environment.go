package codebase

import (
	"bufio"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"

	"github.com/follenfang/lycheedev/internal/codebase/environment"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
)

type environmentCacheManifest struct {
	Schema         string               `json:"schema"`
	SourceMapHash  string               `json:"sourceMapHash"`
	Result         environment.Manifest `json:"result"`
	MappingsSHA256 string               `json:"mappingsSHA256"`
	Coverage       environment.Coverage `json:"coverage"`
}

const maxEnvironmentFactsBytes = 64 << 20

func (b *Browser) researchEnvironment(ctx context.Context, source selection.SourcePin, snapshot string) (environment.Result, string, error) {
	pin := source
	if source.Repository != "wow-ui-source" {
		if snapshot == "" {
			return environment.Result{}, "missing-client-pin", nil
		}
		var err error
		pin, err = pinnedSource(ctx, b.store.Root(), snapshot)
		if err != nil {
			return environment.Result{}, "invalid-client-pin", err
		}
		if pin.Repository != "wow-ui-source" {
			return environment.Result{}, "invalid-client-pin", errors.New("codebase.client_source_pin_required")
		}
	}
	if err := b.EnsureIndex(ctx, pin); err != nil {
		return environment.Result{}, "unavailable", err
	}
	cache, _, err := b.openIndex(ctx, pin)
	if err != nil {
		return environment.Result{}, "unavailable", err
	}
	identity := environment.Identity{Repository: pin.Repository, Commit: pin.ExactCommit, Client: pin.Product, GeneratorVersion: environment.GeneratorVersion, DependencyCommits: map[string]string{}}
	keyHash := sha256.Sum256([]byte(pin.Repository + "\x00" + pin.Product + "\x00" + pin.ExactCommit + "\x00" + cache.manifest.RecordsHash + "\x00" + environment.GeneratorVersion))
	dir := filepath.Join(b.store.Root(), "source", "v1", "environments", hex.EncodeToString(keyHash[:]))
	lease, err := vault.AcquireLease(ctx, filepath.Join(b.store.Root(), "locks"), "source:v1:environment:"+hex.EncodeToString(keyHash[:]))
	if err != nil {
		return environment.Result{}, "unavailable", err
	}
	defer lease.Close()
	if result, err := readEnvironmentCache(ctx, dir, cache.manifest.RecordsHash, identity); err == nil {
		return result, "ready", nil
	}
	docs := map[string][]byte{}
	wanted := []treeFile{}
	records := map[string]sourceRecord{}
	count, total := 0, 0
	err = cache.scan(ctx, func(r sourceRecord) error {
		if r.Kind != "document" || !strings.Contains(strings.ToLower(r.Path), "/blizzard_apidocumentationgenerated/") || !strings.HasSuffix(strings.ToLower(r.Path), ".lua") {
			return nil
		}
		count++
		total += int(r.Bytes)
		if count > 4096 || r.Bytes > 4<<20 || total > 64<<20 {
			return environment.ErrBudget
		}
		wanted = append(wanted, treeFile{path: r.Path, object: r.Object, size: r.Bytes})
		records[r.Path] = r
		return nil
	})
	if err != nil {
		return environment.Result{}, "unavailable", err
	}
	if len(wanted) == 0 {
		return environment.Result{}, "missing-metadata", nil
	}
	if cache.manifest.FixtureRoot != "" {
		for _, file := range wanted {
			data, _, err := cache.document(ctx, file.path)
			if err != nil {
				return environment.Result{}, "unavailable", err
			}
			docs[file.path] = data
		}
	} else {
		err = b.visitDocuments(ctx, pin, wanted, func(file treeFile, data []byte) error {
			h := sha256.Sum256(data)
			if hex.EncodeToString(h[:]) != records[file.path].SHA256 || int64(len(data)) != records[file.path].Bytes {
				return ErrSourceIntegrity
			}
			docs[file.path] = data
			return nil
		})
		if err != nil {
			return environment.Result{}, "unavailable", err
		}
	}
	result, err := environment.Build(ctx, identity, docs, environment.Limits{})
	if err != nil {
		return result, "unavailable", err
	}
	if err := b.writeEnvironmentCache(ctx, dir, cache.manifest.RecordsHash, result); err != nil {
		return result, "unavailable", err
	}
	return result, "ready", nil
}

func readEnvironmentCache(ctx context.Context, dir, mapHash string, identity environment.Identity) (environment.Result, error) {
	var result environment.Result
	f, err := os.Open(filepath.Join(dir, "manifest.json"))
	if err != nil {
		return result, err
	}
	raw, err := io.ReadAll(io.LimitReader(f, (4<<20)+1))
	closeErr := f.Close()
	if err != nil || closeErr != nil {
		return result, errors.Join(err, closeErr)
	}
	if len(raw) > 4<<20 {
		return result, environment.ErrBudget
	}
	var manifest environmentCacheManifest
	if err := json.Unmarshal(raw, &manifest); err != nil {
		return result, err
	}
	if manifest.Schema != "lycheedev.source-environment-cache.v1" || manifest.SourceMapHash != mapHash || manifest.Result.Identity.Repository != identity.Repository || manifest.Result.Identity.Commit != identity.Commit || manifest.Result.Identity.Client != identity.Client || manifest.Result.Identity.GeneratorVersion != identity.GeneratorVersion {
		return result, errors.New("codebase.environment_cache_identity")
	}
	f, err = os.Open(filepath.Join(dir, "api.d.lua"))
	if err != nil {
		return result, err
	}
	result.Definitions, err = io.ReadAll(io.LimitReader(f, (8<<20)+1))
	closeErr = f.Close()
	if err != nil || closeErr != nil {
		return result, errors.Join(err, closeErr)
	}
	if len(result.Definitions) > 8<<20 {
		return result, environment.ErrBudget
	}
	defHash := sha256.Sum256(result.Definitions)
	if hex.EncodeToString(defHash[:]) != manifest.Result.DefinitionsSHA256 {
		return result, errors.New("codebase.environment_cache_content")
	}
	f, err = os.Open(filepath.Join(dir, "facts.jsonl"))
	if err != nil {
		return result, err
	}
	info, err := f.Stat()
	if err != nil || info.Size() > maxEnvironmentFactsBytes {
		f.Close()
		if err != nil {
			return result, err
		}
		return result, environment.ErrBudget
	}
	scanner := bufio.NewScanner(f)
	scanner.Buffer(make([]byte, 64<<10), 1<<20)
	h := sha256.New()
	result.Facts = []environment.APIRecord{}
	var factsBytes int64
	for scanner.Scan() {
		if err := ctx.Err(); err != nil {
			f.Close()
			return result, err
		}
		var record environment.APIRecord
		factsBytes += int64(len(scanner.Bytes()) + 1)
		if factsBytes > maxEnvironmentFactsBytes {
			f.Close()
			return result, environment.ErrBudget
		}
		if err := json.Unmarshal(scanner.Bytes(), &record); err != nil {
			f.Close()
			return result, err
		}
		result.Facts = append(result.Facts, record)
		if len(result.Facts) > 100000 {
			f.Close()
			return result, environment.ErrBudget
		}
	}
	if err := scanner.Err(); err != nil {
		f.Close()
		return result, err
	}
	f.Close()
	factsJSON, err := json.Marshal(result.Facts)
	if err != nil {
		return result, err
	}
	h.Write(factsJSON)
	if hex.EncodeToString(h.Sum(nil)) != manifest.Result.FactsSHA256 {
		return result, errors.New("codebase.environment_cache_content")
	}
	f, err = os.Open(filepath.Join(dir, "mappings.json"))
	if err != nil {
		return result, err
	}
	mappingRaw, err := io.ReadAll(io.LimitReader(f, (16<<20)+1))
	closeErr = f.Close()
	if err != nil || closeErr != nil {
		return result, errors.Join(err, closeErr)
	}
	if len(mappingRaw) > 16<<20 {
		return result, environment.ErrBudget
	}
	mappingHash := sha256.Sum256(mappingRaw)
	if hex.EncodeToString(mappingHash[:]) != manifest.MappingsSHA256 {
		return result, errors.New("codebase.environment_cache_content")
	}
	if err := json.Unmarshal(mappingRaw, &result.Mappings); err != nil {
		return result, err
	}
	result.Manifest = manifest.Result
	result.Coverage = manifest.Coverage
	return result, nil
}

func (b *Browser) writeEnvironmentCache(ctx context.Context, dir, mapHash string, result environment.Result) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	var factsBytes int64
	for _, record := range result.Facts {
		row, err := json.Marshal(record)
		if err != nil {
			return err
		}
		if len(row) > 1<<20 {
			return environment.ErrBudget
		}
		factsBytes += int64(len(row) + 1)
		if factsBytes > maxEnvironmentFactsBytes {
			return environment.ErrBudget
		}
	}
	mappings, err := json.Marshal(result.Mappings)
	if err != nil {
		return err
	}
	if len(mappings) > 16<<20 {
		return environment.ErrBudget
	}
	mappingHash := sha256.Sum256(mappings)
	manifest := environmentCacheManifest{Schema: "lycheedev.source-environment-cache.v1", SourceMapHash: mapHash, Result: result.Manifest, MappingsSHA256: hex.EncodeToString(mappingHash[:]), Coverage: result.Coverage}
	manifestRaw, err := json.Marshal(manifest)
	if err != nil {
		return err
	}
	if len(manifestRaw) > 4<<20 {
		return environment.ErrBudget
	}
	capacity, err := b.reserveSourceCapacity(ctx, int64(len(result.Definitions))+factsBytes+int64(len(mappings))+int64(len(manifestRaw)), dir)
	if err != nil {
		return err
	}
	defer capacity.Close()
	if err := os.MkdirAll(filepath.Dir(dir), 0o755); err != nil {
		return err
	}
	stage, err := os.MkdirTemp(filepath.Dir(dir), ".environment-")
	if err != nil {
		return err
	}
	defer os.RemoveAll(stage)
	if err := os.WriteFile(filepath.Join(stage, "api.d.lua"), result.Definitions, 0o644); err != nil {
		return err
	}
	f, err := os.Create(filepath.Join(stage, "facts.jsonl"))
	if err != nil {
		return err
	}
	w := bufio.NewWriter(f)
	factsBytes = 0
	for _, record := range result.Facts {
		raw, err := json.Marshal(record)
		if err != nil {
			f.Close()
			return err
		}
		if len(raw) > 1<<20 {
			f.Close()
			return environment.ErrBudget
		}
		factsBytes += int64(len(raw) + 1)
		if factsBytes > maxEnvironmentFactsBytes {
			f.Close()
			return environment.ErrBudget
		}
		if _, err := w.Write(raw); err != nil {
			f.Close()
			return err
		}
		if err := w.WriteByte('\n'); err != nil {
			f.Close()
			return err
		}
	}
	if err := w.Flush(); err != nil {
		f.Close()
		return err
	}
	if err := f.Close(); err != nil {
		return err
	}
	if err := os.WriteFile(filepath.Join(stage, "mappings.json"), mappings, 0o644); err != nil {
		return err
	}
	if err := os.WriteFile(filepath.Join(stage, "manifest.json"), manifestRaw, 0o644); err != nil {
		return err
	}
	if err := ctx.Err(); err != nil {
		return err
	}
	// A derived cache is disposable, but a corrupt cache must not mask a
	// freshly rebuilt result. Move it aside under the same managed parent so
	// the new directory can be published without overwriting arbitrary paths.
	quarantine := ""
	if old, err := os.Lstat(dir); err == nil {
		if !old.IsDir() || old.Mode()&os.ModeSymlink != 0 {
			return errors.New("codebase.environment_cache_unsafe_path")
		}
		quarantine, err = os.MkdirTemp(filepath.Dir(dir), ".environment-corrupt-")
		if err != nil {
			return err
		}
		if err := os.Remove(quarantine); err != nil {
			return err
		}
		if err := os.Rename(dir, quarantine); err != nil {
			return err
		}
	} else if !os.IsNotExist(err) {
		return err
	}
	if err := os.Rename(stage, dir); err != nil {
		if quarantine != "" {
			_ = os.Rename(quarantine, dir)
		}
		return fmt.Errorf("codebase.environment_publish: %w", err)
	}
	if quarantine != "" {
		_ = os.RemoveAll(quarantine)
	}
	return nil
}
