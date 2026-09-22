package vault

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"unicode/utf8"
)

// ConfigSchema versions the workspace resource configuration. User preference
// and budget data is never imported from legacy configuration files.
const ConfigSchema = "lycheedev.config.v1"

const (
	// DefaultCacheMaxBytes mirrors the long-standing 20 GiB managed cache
	// budget until the user edits config.json.
	DefaultCacheMaxBytes int64 = 20 << 30
	// MaxCacheMaxBytes keeps a fat-fingered budget from becoming unbounded.
	MaxCacheMaxBytes int64 = 1 << 45
	// MaxDownloadWorkers bounds the fixed worker budget; zero means automatic.
	MaxDownloadWorkers = 256
)

var (
	// ErrConfigFormat rejects corrupt or unknown-shape configuration.
	ErrConfigFormat = errors.New("vault.config_format")
	// ErrConfigSchemaNewer refuses writes against configuration written by a
	// newer toolkit, so an older binary can never overwrite it with defaults.
	ErrConfigSchemaNewer = errors.New("vault.config_schema_newer")
)

// Config is the workspace-level user preference and resource budget document
// stored as config.json in the workspace root.
type Config struct {
	Schema   string         `json:"schema"`
	Cache    CacheBudget    `json:"cache"`
	Download DownloadBudget `json:"download"`
}

// CacheBudget bounds the managed reclaimable cache.
type CacheBudget struct {
	MaxBytes int64 `json:"maxBytes"`
}

// DownloadBudget bounds concurrent download work admitted by this workspace.
type DownloadBudget struct {
	Workers int `json:"workers"`
}

func DefaultConfig() Config {
	return Config{Schema: ConfigSchema, Cache: CacheBudget{MaxBytes: DefaultCacheMaxBytes}, Download: DownloadBudget{}}
}

func (c Config) Validate() error {
	if c.Schema != ConfigSchema {
		return fmt.Errorf("%w: schema %q", ErrConfigFormat, c.Schema)
	}
	if c.Cache.MaxBytes < 0 || c.Cache.MaxBytes > MaxCacheMaxBytes {
		return fmt.Errorf("%w: cache.maxBytes out of range", ErrConfigFormat)
	}
	if c.Download.Workers < 0 || c.Download.Workers > MaxDownloadWorkers {
		return fmt.Errorf("%w: download.workers out of range", ErrConfigFormat)
	}
	return nil
}

// ReadConfig returns the workspace configuration. A missing file yields the
// documented defaults without creating anything; a newer or corrupt file is
// reported precisely and never reinterpreted as defaults.
func ReadConfig(ctx context.Context, root string) (Config, error) {
	if err := ctx.Err(); err != nil {
		return Config{}, err
	}
	if _, err := OpenStore(root); err != nil {
		return Config{}, err
	}
	return readConfigFile(filepath.Join(mustStoreRoot(root), "config.json"))
}

// WriteConfig replaces the whole configuration atomically under the workspace
// config lease. The incoming document must be complete and valid.
func WriteConfig(ctx context.Context, root string, config Config) (Config, error) {
	return UpdateConfig(ctx, root, func(current *Config) error {
		*current = config
		return nil
	})
}

// UpdateConfig performs a validated read-modify-write under the config lease,
// publishing atomically. It refuses to write over a newer or corrupt schema.
func UpdateConfig(ctx context.Context, root string, mutate func(*Config) error) (Config, error) {
	var zero Config
	if mutate == nil {
		return zero, errors.New("vault: missing config mutation")
	}
	store, err := OpenStore(root)
	if err != nil {
		return zero, err
	}
	lease, err := AcquireLease(ctx, filepath.Join(store.Root(), "locks"), "config")
	if err != nil {
		return zero, err
	}
	defer lease.Close()
	current, err := readConfigFile(filepath.Join(store.Root(), "config.json"))
	if err != nil {
		return zero, err
	}
	if err := mutate(&current); err != nil {
		return zero, err
	}
	if err := current.Validate(); err != nil {
		return zero, err
	}
	if err := ctx.Err(); err != nil {
		return zero, err
	}
	raw, err := json.MarshalIndent(current, "", "  ")
	if err != nil {
		return zero, err
	}
	if len(raw) > 65535 {
		return zero, fmt.Errorf("%w: document too large", ErrConfigFormat)
	}
	if err := writeDurableReplace(store.Root(), "config.json", append(raw, '\n')); err != nil {
		return zero, err
	}
	return current, nil
}

func readConfigFile(path string) (Config, error) {
	var config Config
	info, err := os.Lstat(path)
	if errors.Is(err, os.ErrNotExist) {
		return DefaultConfig(), nil
	}
	if err != nil {
		return config, err
	}
	if !info.Mode().IsRegular() || info.Size() > 65536 {
		return config, fmt.Errorf("%w: document shape", ErrConfigFormat)
	}
	f, err := os.Open(path)
	if err != nil {
		return config, err
	}
	defer f.Close()
	raw, err := io.ReadAll(io.LimitReader(f, 65537))
	if err != nil {
		return config, err
	}
	if len(raw) > 65536 || !utf8.Valid(raw) {
		return config, fmt.Errorf("%w: document encoding", ErrConfigFormat)
	}
	schema, err := peekConfigSchema(raw)
	if err != nil {
		return config, err
	}
	if schema != ConfigSchema {
		if configSchemaNewer(schema) {
			return config, fmt.Errorf("%w: %s", ErrConfigSchemaNewer, schema)
		}
		return config, fmt.Errorf("%w: schema %q", ErrConfigFormat, schema)
	}
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&config); err != nil {
		return config, fmt.Errorf("%w: %v", ErrConfigFormat, err)
	}
	if err := decoder.Decode(new(any)); !errors.Is(err, io.EOF) {
		return config, fmt.Errorf("%w: trailing document data", ErrConfigFormat)
	}
	if err := config.Validate(); err != nil {
		return config, err
	}
	return config, nil
}

func peekConfigSchema(raw []byte) (string, error) {
	var head struct {
		Schema string `json:"schema"`
	}
	if err := json.Unmarshal(raw, &head); err != nil {
		return "", fmt.Errorf("%w: %v", ErrConfigFormat, err)
	}
	if head.Schema == "" {
		return "", fmt.Errorf("%w: missing schema", ErrConfigFormat)
	}
	return head.Schema, nil
}

func configSchemaNewer(schema string) bool {
	const prefix = "lycheedev.config.v"
	if len(schema) <= len(prefix) || schema[:len(prefix)] != prefix {
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

// mustStoreRoot resolves the already-validated store root for a caller path.
func mustStoreRoot(root string) string {
	abs, err := filepath.Abs(root)
	if err != nil {
		return root
	}
	return abs
}
