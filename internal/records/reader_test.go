package records

import (
	"context"
	"crypto/md5"
	"encoding/hex"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
)

func TestChooseFileSelectsOnlyAnExactFileAndLocale(t *testing.T) {
	want := RootRecord{FileDataID: 11, ContentKey: strings.Repeat("a", 32), LocaleMask: 0x10}
	entries := []RootRecord{
		{FileDataID: 10, ContentKey: strings.Repeat("b", 32), LocaleMask: 0x10},
		{FileDataID: 11, ContentKey: strings.Repeat("c", 32), LocaleMask: 0x2},
		want,
		{FileDataID: 12, ContentKey: strings.Repeat("d", 32), LocaleMask: 0x10},
	}

	got, err := chooseFile(entries, want.FileDataID, want.LocaleMask)
	if err != nil {
		t.Fatal(err)
	}
	if got != want {
		t.Fatalf("selected %+v, want %+v", got, want)
	}
}

func TestChooseFileRejectsMissingExactFileOrLocale(t *testing.T) {
	entries := []RootRecord{
		{FileDataID: 11, ContentKey: strings.Repeat("a", 32), LocaleMask: 0x2},
		{FileDataID: 12, ContentKey: strings.Repeat("b", 32), LocaleMask: 0x10},
	}
	for name, idLocale := range map[string][2]uint32{
		"zero id":        {0, 0x2},
		"missing id":     {13, 0x2},
		"missing locale": {11, 0x10},
	} {
		t.Run(name, func(t *testing.T) {
			if _, err := chooseFile(entries, idLocale[0], idLocale[1]); !errors.Is(err, ErrContentMissing) {
				t.Fatalf("error %v, want %v", err, ErrContentMissing)
			}
		})
	}
}

func TestChooseFileRejectsDuplicateVariantsEvenWhenCKeysMatch(t *testing.T) {
	entries := []RootRecord{
		{FileDataID: 11, ContentKey: strings.Repeat("a", 32), LocaleMask: 0x10, Group: 1},
		{FileDataID: 11, ContentKey: strings.Repeat("a", 32), LocaleMask: 0x10, Group: 2},
	}

	if _, err := chooseFile(entries, 11, 0x10); !errors.Is(err, ErrFileAmbiguous) {
		t.Fatalf("error %v, want %v", err, ErrFileAmbiguous)
	}
}

func TestChooseFileDoesNotFallBackToAnotherLocale(t *testing.T) {
	entries := []RootRecord{
		{FileDataID: 11, ContentKey: strings.Repeat("a", 32), LocaleMask: 0x2},
	}

	if _, err := chooseFile(entries, 11, 0x10); !errors.Is(err, ErrContentMissing) {
		t.Fatalf("error %v, want %v", err, ErrContentMissing)
	}
}

func TestReaderRejectsInvalidBudgets(t *testing.T) {
	reader := OpenReader(nil)
	pin := validReaderPin()
	for name, query := range map[string]FileQuery{
		"zero metadata":      {Installation: "unused", FileDataID: 11, MetadataBytes: 0, ContentBytes: 1},
		"metadata too large": {Installation: "unused", FileDataID: 11, MetadataBytes: 1<<30 + 1, ContentBytes: 1},
		"zero content":       {Installation: "unused", FileDataID: 11, MetadataBytes: 1, ContentBytes: 0},
		"content too large":  {Installation: "unused", FileDataID: 11, MetadataBytes: 1, ContentBytes: 1<<40 + 1},
	} {
		t.Run(name, func(t *testing.T) {
			if _, err := reader.ReadFile(context.Background(), pin, query); !errors.Is(err, ErrMetadataLimit) {
				t.Fatalf("error %v, want %v", err, ErrMetadataLimit)
			}
		})
	}
}

func TestReaderChecksContextBeforeOtherInputs(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	cancel()

	if _, err := OpenReader(nil).ReadFile(ctx, selection.DataPin{}, FileQuery{}); !errors.Is(err, context.Canceled) {
		t.Fatalf("error %v, want %v", err, context.Canceled)
	}
}

func TestReaderRejectsNilStore(t *testing.T) {
	query := FileQuery{Installation: "unused", FileDataID: 11, MetadataBytes: 1, ContentBytes: 1}
	if _, err := OpenReader(nil).ReadFile(context.Background(), validReaderPin(), query); !errors.Is(err, ErrMetadataLimit) {
		t.Fatalf("error %v, want %v", err, ErrMetadataLimit)
	}
}

func TestReaderRejectsExactPinConfigurationConflict(t *testing.T) {
	installation, actualBuild, actualCDN := makeReaderInstallation(t)
	store, err := vault.Initialize(context.Background(), filepath.Join(t.TempDir(), "workspace"))
	if err != nil {
		t.Fatal(err)
	}
	reader := OpenReader(store)
	query := FileQuery{Installation: installation, FileDataID: 11, MetadataBytes: 1, ContentBytes: 1}

	for name, pin := range map[string]selection.DataPin{
		"build config": func() selection.DataPin {
			p := validReaderPin()
			p.BuildConfig = differentReaderKey(actualBuild)
			p.CDNConfig = actualCDN
			return p
		}(),
		"cdn config": func() selection.DataPin {
			p := validReaderPin()
			p.BuildConfig = actualBuild
			p.CDNConfig = differentReaderKey(actualCDN)
			return p
		}(),
	} {
		t.Run(name, func(t *testing.T) {
			if _, err := reader.ReadFile(context.Background(), pin, query); !errors.Is(err, ErrPinnedBuildChanged) {
				t.Fatalf("error %v, want %v", err, ErrPinnedBuildChanged)
			}
		})
	}
}

func validReaderPin() selection.DataPin {
	return selection.DataPin{
		Product:          "retail",
		Region:           "cn",
		FullBuild:        "12.1.0.69875",
		BuildConfig:      strings.Repeat("a", 32),
		CDNConfig:        strings.Repeat("b", 32),
		Language:         "zhCN",
		DefinitionCommit: strings.Repeat("c", 40),
	}
}

func makeReaderInstallation(t *testing.T) (root, buildKey, cdnKey string) {
	t.Helper()
	root = t.TempDir()
	buildRaw := []byte(fmt.Sprintf("root = %s\nencoding = %s %s\nencoding-size = 1 1\nbuild-uid = wow\n", strings.Repeat("1", 32), strings.Repeat("2", 32), strings.Repeat("3", 32)))
	cdnRaw := []byte("archives = one\n")
	buildKey = readerMD5Key(buildRaw)
	cdnKey = readerMD5Key(cdnRaw)
	catalog := fmt.Sprintf("Product!STRING:0|Version!STRING:0|Build Key!HEX:16|CDN Key!HEX:16|Active!DEC:1\nwow|12.1.0.69875|%s|%s|1\n", buildKey, cdnKey)
	if err := os.WriteFile(filepath.Join(root, ".build.info"), []byte(catalog), 0o600); err != nil {
		t.Fatal(err)
	}
	writeReaderConfig(t, root, buildKey, buildRaw)
	writeReaderConfig(t, root, cdnKey, cdnRaw)
	return root, buildKey, cdnKey
}

func writeReaderConfig(t *testing.T, root, key string, raw []byte) {
	t.Helper()
	dir := filepath.Join(root, "Data", "config", key[:2], key[2:4])
	if err := os.MkdirAll(dir, 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(dir, key), raw, 0o600); err != nil {
		t.Fatal(err)
	}
}

func readerMD5Key(raw []byte) string {
	sum := md5.Sum(raw)
	return hex.EncodeToString(sum[:])
}

func differentReaderKey(key string) string {
	if key[0] != 'a' {
		return "a" + key[1:]
	}
	return "b" + key[1:]
}
