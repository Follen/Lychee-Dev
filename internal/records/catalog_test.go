package records_test

import (
	"bytes"
	"context"
	"crypto/md5"
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/records"
)

const catalogHeader = "Product!STRING:0|Version!STRING:0|Build Key!HEX:16|CDN Key!HEX:16|Active!DEC:1\n"

type catalogFixture struct {
	root      string
	product   string
	fullBuild string
	buildKey  string
	cdnKey    string
	buildRaw  []byte
	cdnRaw    []byte
}

func md5Key(data []byte) string {
	sum := md5.Sum(data)
	return hex.EncodeToString(sum[:])
}

func sha256Hex(data []byte) string {
	sum := sha256.Sum256(data)
	return hex.EncodeToString(sum[:])
}

func writeCatalog(t *testing.T, root, body string) {
	t.Helper()
	writeCatalogText(t, root, catalogHeader+body)
}

func writeCatalogText(t *testing.T, root, text string) {
	t.Helper()
	if err := os.WriteFile(filepath.Join(root, ".build.info"), []byte(text), 0o600); err != nil {
		t.Fatal(err)
	}
}

func writeConfig(t *testing.T, root, key string, raw []byte) {
	t.Helper()
	if len(key) < 4 {
		t.Fatalf("config key is too short: %q", key)
	}
	dir := filepath.Join(root, "Data", "config", key[:2], key[2:4])
	if err := os.MkdirAll(dir, 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(dir, key), raw, 0o600); err != nil {
		t.Fatal(err)
	}
}

func makeFixture(t *testing.T, buildRaw, cdnRaw []byte) catalogFixture {
	t.Helper()
	root := t.TempDir()
	fixture := catalogFixture{
		root:      root,
		product:   "wow",
		fullBuild: "12.1.0.69875",
		buildKey:  md5Key(buildRaw),
		cdnKey:    md5Key(cdnRaw),
		buildRaw:  buildRaw,
		cdnRaw:    cdnRaw,
	}
	writeCatalog(t, root, fmt.Sprintf("%s|%s|%s|%s|1\n", fixture.product, fixture.fullBuild, fixture.buildKey, fixture.cdnKey))
	writeConfig(t, root, fixture.buildKey, buildRaw)
	writeConfig(t, root, fixture.cdnKey, cdnRaw)
	return fixture
}

func validBuildConfig(product string) []byte {
	rootKey := md5Key([]byte("root content"))
	encodingContentKey := md5Key([]byte("encoding content"))
	encodingKey := md5Key([]byte("encoding"))
	return []byte(fmt.Sprintf(
		"# build config\r\n\r\nroot = %s\r\nencoding = %s %s\r\nencoding-size = 123 456\r\nbuild-uid = %s\r\nunknown = first second\r\n",
		rootKey, encodingContentKey, encodingKey, product))
}

func validCDNConfig() []byte {
	return []byte("# cdn config\r\n\r\narchives = one two\r\nunknown-cdn = alpha beta\r\n")
}

func TestReadInstallCatalogAndResolveLocalBuild(t *testing.T) {
	buildRaw := validBuildConfig("wow")
	cdnRaw := validCDNConfig()
	fixture := makeFixture(t, buildRaw, cdnRaw)

	otherBuildKey := md5Key([]byte("other build"))
	otherCDNKey := md5Key([]byte("other cdn"))
	inactiveBuildKey := md5Key([]byte("inactive build"))
	inactiveCDNKey := md5Key([]byte("inactive cdn"))
	writeCatalog(t, fixture.root, strings.Join([]string{
		fmt.Sprintf("unknown_product|%s|%s|%s|1", fixture.fullBuild, otherBuildKey, otherCDNKey),
		fmt.Sprintf("%s|%s|%s|%s|0", fixture.product, fixture.fullBuild, inactiveBuildKey, inactiveCDNKey),
		fmt.Sprintf("%s|12.1.0.69874|%s|%s|1", fixture.product, otherBuildKey, otherCDNKey),
		fmt.Sprintf("%s|%s|%s|%s|1", fixture.product, fixture.fullBuild, fixture.buildKey, fixture.cdnKey),
	}, "\n")+"\n")

	ctx := context.Background()
	gotCatalog, err := records.ReadInstallCatalog(ctx, fixture.root)
	if err != nil {
		t.Fatal(err)
	}
	wantCatalog := []records.InstalledBuild{
		{Product: "unknown_product", FullBuild: fixture.fullBuild, BuildConfig: otherBuildKey, CDNConfig: otherCDNKey, Active: true},
		{Product: fixture.product, FullBuild: fixture.fullBuild, BuildConfig: inactiveBuildKey, CDNConfig: inactiveCDNKey, Active: false},
		{Product: fixture.product, FullBuild: "12.1.0.69874", BuildConfig: otherBuildKey, CDNConfig: otherCDNKey, Active: true},
		{Product: fixture.product, FullBuild: fixture.fullBuild, BuildConfig: fixture.buildKey, CDNConfig: fixture.cdnKey, Active: true},
	}
	if !reflect.DeepEqual(gotCatalog, wantCatalog) {
		t.Fatalf("catalog mismatch:\n got %#v\nwant %#v", gotCatalog, wantCatalog)
	}

	metadata, err := records.ResolveLocalBuild(ctx, fixture.root, fixture.product, fixture.fullBuild)
	if err != nil {
		t.Fatal(err)
	}
	if metadata.Installed != wantCatalog[3] {
		t.Fatalf("selected build mismatch: %#v", metadata.Installed)
	}
	if metadata.RootContentKey != md5Key([]byte("root content")) {
		t.Fatalf("root content key: %q", metadata.RootContentKey)
	}
	if metadata.EncodingContentKey != md5Key([]byte("encoding content")) || metadata.EncodingKey != md5Key([]byte("encoding")) {
		t.Fatalf("encoding keys: content=%q encoding=%q", metadata.EncodingContentKey, metadata.EncodingKey)
	}
	if metadata.ContentBytes != 123 || metadata.EncodingBytes != 456 {
		t.Fatalf("encoding sizes: content=%d encoding=%d", metadata.ContentBytes, metadata.EncodingBytes)
	}
	if !bytes.Equal(metadata.BuildDocument.Raw, buildRaw) || !bytes.Equal(metadata.CDNDocument.Raw, cdnRaw) {
		t.Fatal("config raw bytes were not preserved")
	}
	if metadata.BuildDocument.Key != fixture.buildKey || metadata.CDNDocument.Key != fixture.cdnKey {
		t.Fatalf("config keys: build=%q cdn=%q", metadata.BuildDocument.Key, metadata.CDNDocument.Key)
	}
	if metadata.BuildDocument.SHA256 != sha256Hex(buildRaw) || metadata.CDNDocument.SHA256 != sha256Hex(cdnRaw) {
		t.Fatalf("config SHA-256 values were not computed from raw bytes")
	}
	if !reflect.DeepEqual(metadata.BuildDocument.Fields["unknown"], []string{"first", "second"}) {
		t.Fatalf("unknown build field: %#v", metadata.BuildDocument.Fields["unknown"])
	}
	if !reflect.DeepEqual(metadata.CDNDocument.Fields["unknown-cdn"], []string{"alpha", "beta"}) {
		t.Fatalf("unknown CDN field: %#v", metadata.CDNDocument.Fields["unknown-cdn"])
	}
}

func TestResolveLocalBuildRequiresExactActiveSelection(t *testing.T) {
	buildRaw := validBuildConfig("wow")
	cdnRaw := validCDNConfig()
	fixture := makeFixture(t, buildRaw, cdnRaw)
	otherBuildKey := md5Key([]byte("other build"))
	otherCDNKey := md5Key([]byte("other cdn"))
	writeCatalog(t, fixture.root, strings.Join([]string{
		fmt.Sprintf("wow|%s|%s|%s|0", fixture.fullBuild, fixture.buildKey, fixture.cdnKey),
		fmt.Sprintf("wow|12.1.0.69874|%s|%s|1", otherBuildKey, otherCDNKey),
		fmt.Sprintf("wow_classic|%s|%s|%s|1", fixture.fullBuild, otherBuildKey, otherCDNKey),
	}, "\n")+"\n")

	if _, err := records.ResolveLocalBuild(context.Background(), fixture.root, "wow", fixture.fullBuild); err == nil {
		t.Fatal("inactive exact row resolved through another build or product")
	}
	if _, err := records.ResolveLocalBuild(context.Background(), fixture.root, "wow", "12.1.0.69873"); err == nil {
		t.Fatal("nonexistent build resolved through a nearby build")
	}

	writeCatalog(t, fixture.root, strings.Join([]string{
		fmt.Sprintf("wow|%s|%s|%s|1", fixture.fullBuild, fixture.buildKey, fixture.cdnKey),
		fmt.Sprintf("wow|%s|%s|%s|1", fixture.fullBuild, fixture.buildKey, fixture.cdnKey),
	}, "\n")+"\n")
	if _, err := records.ResolveLocalBuild(context.Background(), fixture.root, "wow", fixture.fullBuild); err == nil {
		t.Fatal("ambiguous active rows were accepted")
	}
}

func TestResolveLocalBuildRejectsDuplicateConfigEntries(t *testing.T) {
	for _, test := range []struct {
		name     string
		buildRaw []byte
		cdnRaw   []byte
	}{
		{
			name:     "duplicate build field",
			buildRaw: append(validBuildConfig("wow"), []byte("root = "+md5Key([]byte("another root"))+"\r\n")...),
			cdnRaw:   validCDNConfig(),
		},
		{
			name:     "duplicate CDN field",
			buildRaw: validBuildConfig("wow"),
			cdnRaw:   append(validCDNConfig(), []byte("archives = three\r\n")...),
		},
	} {
		t.Run(test.name, func(t *testing.T) {
			fixture := makeFixture(t, test.buildRaw, test.cdnRaw)
			if _, err := records.ResolveLocalBuild(context.Background(), fixture.root, fixture.product, fixture.fullBuild); err == nil {
				t.Fatal("duplicate config entry was accepted")
			}
		})
	}
}

func TestCatalogRejectsInvalidInput(t *testing.T) {
	validBuildKey := md5Key([]byte("build"))
	validCDNKey := md5Key([]byte("cdn"))
	for _, test := range []struct {
		name string
		text string
	}{
		{name: "duplicate header", text: "Product!STRING:0|Product!STRING:1|Version!STRING:0|Build Key!HEX:16|CDN Key!HEX:16|Active!DEC:1\n"},
		{name: "invalid field count", text: catalogHeader + fmt.Sprintf("wow|12.1.0.69875|%s|%s\n", validBuildKey, validCDNKey)},
		{name: "bad build digest", text: catalogHeader + "wow|12.1.0.69875|not-md5|" + validCDNKey + "|1\n"},
		{name: "bad full build", text: catalogHeader + "wow|not-a-build|" + validBuildKey + "|" + validCDNKey + "|1\n"},
		{name: "bad active value", text: catalogHeader + "wow|12.1.0.69875|" + validBuildKey + "|" + validCDNKey + "|maybe\n"},
		{name: "duplicate required header", text: "Product!STRING:0|Version!STRING:0|Build Key!HEX:16|CDN Key!HEX:16|Active!DEC:1|Active!DEC:1\n"},
	} {
		t.Run(test.name, func(t *testing.T) {
			root := t.TempDir()
			writeCatalogText(t, root, test.text)
			if _, err := records.ReadInstallCatalog(context.Background(), root); err == nil {
				t.Fatal("invalid catalog input was accepted")
			}
		})
	}
}

func TestResolveLocalBuildRejectsMalformedConfigValues(t *testing.T) {
	validCDN := validCDNConfig()
	validRoot := md5Key([]byte("root content"))
	validEncodingContent := md5Key([]byte("encoding content"))
	validEncoding := md5Key([]byte("encoding"))
	for _, test := range []struct {
		name      string
		buildBody string
	}{
		{name: "root requires exactly one MD5", buildBody: fmt.Sprintf("root = %s %s\nencoding = %s %s\nencoding-size = 123 456\nbuild-uid = wow\n", validRoot, validRoot, validEncodingContent, validEncoding)},
		{name: "encoding requires exactly two MD5 values", buildBody: fmt.Sprintf("root = %s\nencoding = %s\nencoding-size = 123 456\nbuild-uid = wow\n", validRoot, validEncodingContent)},
		{name: "encoding size requires positive integers", buildBody: fmt.Sprintf("root = %s\nencoding = %s %s\nencoding-size = 0 -1\nbuild-uid = wow\n", validRoot, validEncodingContent, validEncoding)},
		{name: "build uid must match product", buildBody: fmt.Sprintf("root = %s\nencoding = %s %s\nencoding-size = 123 456\nbuild-uid = wow_classic\n", validRoot, validEncodingContent, validEncoding)},
		{name: "malformed embedded line", buildBody: fmt.Sprintf("root = %s\nthis is not a config entry\nencoding = %s %s\nencoding-size = 123 456\nbuild-uid = wow\n", validRoot, validEncodingContent, validEncoding)},
	} {
		t.Run(test.name, func(t *testing.T) {
			fixture := makeFixture(t, []byte(test.buildBody), validCDN)
			if _, err := records.ResolveLocalBuild(context.Background(), fixture.root, fixture.product, fixture.fullBuild); err == nil {
				t.Fatal("malformed config input was accepted")
			}
		})
	}
}

func TestResolveLocalBuildRejectsConfigHashMismatch(t *testing.T) {
	buildRaw := validBuildConfig("wow")
	cdnRaw := validCDNConfig()
	root := t.TempDir()
	buildKey := md5Key(buildRaw)
	cdnKey := md5Key(cdnRaw)
	writeCatalog(t, root, fmt.Sprintf("wow|12.1.0.69875|%s|%s|1\n", buildKey, cdnKey))
	writeConfig(t, root, buildKey, append([]byte{}, buildRaw...))
	writeConfig(t, root, cdnKey, append(cdnRaw, '\n'))

	if _, err := records.ResolveLocalBuild(context.Background(), root, "wow", "12.1.0.69875"); err == nil {
		t.Fatal("config with a raw-byte MD5 mismatch was accepted")
	}
}
