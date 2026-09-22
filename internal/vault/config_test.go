package vault

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"testing"
)

func TestConfigRoundTripAndValidation(t *testing.T) {
	ctx := context.Background()
	root := t.TempDir()
	if _, err := Initialize(ctx, root); err != nil {
		t.Fatal(err)
	}
	defaults, err := ReadConfig(ctx, root)
	if err != nil || defaults != DefaultConfig() {
		t.Fatalf("ReadConfig() = %+v, %v", defaults, err)
	}
	if _, err := os.Lstat(filepath.Join(root, "config.json")); !errors.Is(err, os.ErrNotExist) {
		t.Fatal("ReadConfig created a file for missing configuration")
	}

	written, err := WriteConfig(ctx, root, Config{
		Schema: ConfigSchema, Cache: CacheBudget{MaxBytes: 4096}, Download: DownloadBudget{Workers: 8},
	})
	if err != nil {
		t.Fatalf("WriteConfig() error = %v", err)
	}
	if written.Cache.MaxBytes != 4096 || written.Download.Workers != 8 {
		t.Fatalf("WriteConfig() = %+v", written)
	}
	read, err := ReadConfig(ctx, root)
	if err != nil || read != written {
		t.Fatalf("ReadConfig() = %+v, %v; want %+v", read, err, written)
	}

	updated, err := UpdateConfig(ctx, root, func(config *Config) error {
		config.Download.Workers = 16
		return nil
	})
	if err != nil || updated.Download.Workers != 16 || updated.Cache.MaxBytes != 4096 {
		t.Fatalf("UpdateConfig() = %+v, %v", updated, err)
	}

	for _, invalid := range []Config{
		{Schema: "lycheedev.config.v1", Cache: CacheBudget{MaxBytes: -1}},
		{Schema: "lycheedev.config.v1", Cache: CacheBudget{MaxBytes: MaxCacheMaxBytes + 1}},
		{Schema: "lycheedev.config.v1", Download: DownloadBudget{Workers: -2}},
		{Schema: "lycheedev.config.v1", Download: DownloadBudget{Workers: MaxDownloadWorkers + 1}},
		{Schema: "other.v1"},
	} {
		if _, err := WriteConfig(ctx, root, invalid); !errors.Is(err, ErrConfigFormat) {
			t.Fatalf("WriteConfig(%+v) error = %v, want %v", invalid, err, ErrConfigFormat)
		}
	}
	after, err := ReadConfig(ctx, root)
	if err != nil || after != updated {
		t.Fatalf("invalid writes changed config: %+v, %v", after, err)
	}
}

func TestConfigRefusesUnknownFieldsOnWrite(t *testing.T) {
	ctx := context.Background()
	root := t.TempDir()
	if _, err := Initialize(ctx, root); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(root, "config.json")
	if err := os.WriteFile(path, []byte(`{"schema":"lycheedev.config.v1","cache":{"maxBytes":1},"download":{"workers":1},"extra":true}`+"\n"), 0600); err != nil {
		t.Fatal(err)
	}
	if _, err := ReadConfig(ctx, root); !errors.Is(err, ErrConfigFormat) {
		t.Fatalf("ReadConfig() error = %v, want %v", err, ErrConfigFormat)
	}
	if _, err := UpdateConfig(ctx, root, func(*Config) error { return nil }); !errors.Is(err, ErrConfigFormat) {
		t.Fatalf("UpdateConfig() error = %v, want %v", err, ErrConfigFormat)
	}
}
