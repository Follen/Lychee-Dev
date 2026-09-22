package records_test

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/records"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
)

func targetFixture(t *testing.T, product, build, folder string) (catalogFixture, string, *vault.Store, *vault.Metadata) {
	t.Helper()
	fixture := makeFixture(t, validBuildConfig(product), validCDNConfig())
	fixture.product, fixture.fullBuild = product, build
	writeCatalog(t, fixture.root, fmt.Sprintf("%s|%s|%s|%s|1\n", product, build, fixture.buildKey, fixture.cdnKey))
	client := filepath.Join(fixture.root, folder)
	if err := os.Mkdir(client, 0700); err != nil {
		t.Fatal(err)
	}
	for name, value := range map[string]string{".flavor.info": product, "version.txt": build} {
		if err := os.WriteFile(filepath.Join(client, name), []byte(value), 0600); err != nil {
			t.Fatal(err)
		}
	}
	store, err := vault.Initialize(context.Background(), filepath.Join(t.TempDir(), "workspace"))
	if err != nil {
		t.Fatal(err)
	}
	metadata, err := store.OpenMetadata(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { metadata.Close() })
	return fixture, client, store, metadata
}

func TestResolveLocalTargetFourClients(t *testing.T) {
	for _, baseline := range selection.VerifiedClientBaselines() {
		t.Run(baseline.Product, func(t *testing.T) {
			folder := "selected-client"
			if baseline.Product == "forever" {
				folder = "_classic_beta_"
			}
			fixture, client, store, metadata := targetFixture(t, baseline.ProductCode, baseline.BuildSeries+".99999", folder)
			ctx := context.Background()
			request := records.LocalTargetRequest{Installation: client, Region: "cn", Locale: "zhCN", Definitions: strings.Repeat("d", 40), Offline: true}
			first, err := records.ResolveLocalTarget(ctx, store.Root(), request)
			if err != nil {
				t.Fatal(err)
			}
			if first.Pin.Data == nil || first.Pin.Data.Product != baseline.Product || first.Pin.Data.FullBuild != fixture.fullBuild || first.Pin.Data.BuildConfig != fixture.buildKey || first.Pin.Data.CDNConfig != fixture.cdnKey || first.Pin.Data.DefinitionCommit != request.Definitions || first.Client.Directory != client || first.Installation != fixture.root {
				t.Fatalf("incorrect resolved target: %+v", first)
			}
			second, err := records.ResolveLocalTarget(ctx, store.Root(), request)
			if err != nil || second.Pin.ID != first.Pin.ID {
				t.Fatal("same identities produced a different pin", err)
			}
			_, raw, err := evidence.OpenArchive(store, metadata).FetchCapture(ctx, first.Capture.ID, 65536)
			if err != nil {
				t.Fatal(err)
			}
			var proof struct {
				Pin            selection.PinnedSet `json:"pin"`
				CatalogSHA256  string              `json:"catalogSHA256"`
				Configurations []vault.BlobRef     `json:"configurations"`
			}
			if err := json.Unmarshal(raw, &proof); err != nil || proof.Pin.ID != first.Pin.ID || len(proof.Configurations) != 2 || proof.CatalogSHA256 == "" {
				t.Fatal("incomplete target evidence", err)
			}
			for _, ref := range proof.Configurations {
				if err := store.VerifyBlob(ctx, ref); err != nil {
					t.Fatal(err)
				}
			}
			if !first.Capture.Complete || first.Capture.Provenance.Snapshot != first.Pin.ID {
				t.Fatal("wrong capture identity")
			}
		})
	}
}

func TestResolveLocalTargetExtendsSourceWithoutChangingParent(t *testing.T) {
	_, client, store, metadata := targetFixture(t, "wow", "12.1.0.69875", "_retail_")
	ctx := context.Background()
	pinner := selection.OpenPinner(metadata)
	parent, err := pinner.PinSelection(ctx, selection.SelectionSpec{Source: &selection.SourcePin{Repository: "wow-ui-source", Product: "retail", ExactCommit: strings.Repeat("b", 40), ParserRevision: "1"}})
	if err != nil {
		t.Fatal(err)
	}
	request := records.LocalTargetRequest{Installation: client, Region: "cn", Locale: "zhCN", Definitions: strings.Repeat("d", 40), Parent: parent.ID, Offline: true}
	resolved, err := records.ResolveLocalTarget(ctx, store.Root(), request)
	if err != nil {
		t.Fatal(err)
	}
	if resolved.Pin.Parent != parent.ID || *resolved.Pin.Source != *parent.Source || resolved.Pin.Data == nil {
		t.Fatal("parent not preserved", resolved.Pin)
	}
	original, err := pinner.ReadPinnedSet(ctx, parent.ID)
	if err != nil || original.Data != nil {
		t.Fatal("mutated original source pin", err)
	}
	request.Parent, request.Definitions = resolved.Pin.ID, ""
	continued, err := records.ResolveLocalTarget(ctx, store.Root(), request)
	if err != nil || continued.Pin.Data.DefinitionCommit != resolved.Pin.Data.DefinitionCommit {
		t.Fatal("resolved parent followed a mutable definition ref", err)
	}
	request.Definitions = strings.Repeat("e", 40)
	if _, err := records.ResolveLocalTarget(ctx, store.Root(), request); err == nil {
		t.Fatal("overrode fixed definition identity")
	}
}

func TestResolveLocalTargetDoesNotPublishConflictingOrCorruptMetadata(t *testing.T) {
	for _, mode := range []string{"catalog-build", "configuration", "flavor", "inactive", "ambiguous", "bad-locale", "bad-region", "cancelled"} {
		t.Run(mode, func(t *testing.T) {
			fixture, client, store, metadata := targetFixture(t, "wow", "12.1.0.69875", "_retail_")
			ctx := context.Background()
			request := records.LocalTargetRequest{Installation: client, Region: "cn", Locale: "zhCN", Definitions: strings.Repeat("d", 40), Offline: true}
			row := fmt.Sprintf("wow|12.1.0.69875|%s|%s|1\n", fixture.buildKey, fixture.cdnKey)
			switch mode {
			case "catalog-build":
				writeCatalog(t, fixture.root, strings.ReplaceAll(row, "69875", "69876"))
			case "configuration":
				writeConfig(t, fixture.root, fixture.buildKey, []byte("modified"))
			case "flavor":
				if err := os.WriteFile(filepath.Join(client, ".flavor.info"), []byte("wow_classic"), 0600); err != nil {
					t.Fatal(err)
				}
			case "inactive":
				writeCatalog(t, fixture.root, strings.TrimSuffix(row, "1\n")+"0\n")
			case "ambiguous":
				writeCatalog(t, fixture.root, row+row)
			case "bad-locale":
				request.Locale = "guess"
			case "bad-region":
				request.Region = "guess"
			case "cancelled":
				cancelled, cancel := context.WithCancel(ctx)
				cancel()
				ctx = cancelled
			}
			if _, err := records.ResolveLocalTarget(ctx, store.Root(), request); err == nil {
				t.Fatal("accepted", mode)
			}
			pins, err := metadata.ListDocuments(context.Background(), "pin/", "", 2)
			if err != nil || len(pins) != 0 {
				t.Fatal("invalid source published a pin", err)
			}
		})
	}
}

func TestClientDataReadChecksSelectedClientBeforeArchives(t *testing.T) {
	fixture, client, store, _ := targetFixture(t, "wow", "12.1.0.69875", "_retail_")
	request := records.LocalTargetRequest{Installation: client, Region: "cn", Locale: "zhCN", Definitions: strings.Repeat("d", 40), Offline: true}
	target, err := records.ResolveLocalTarget(context.Background(), store.Root(), request)
	if err != nil {
		t.Fatal(err)
	}
	// Tiny payload budgets stop after metadata, proving both path forms reach
	// the same selected archives without inventing fake game payloads.
	for _, directory := range []string{fixture.root, client} {
		_, err := records.OpenReader(store).ReadFile(context.Background(), *target.Pin.Data, records.FileQuery{Installation: directory, FileDataID: 1, MetadataBytes: 1, ContentBytes: 1})
		if !errors.Is(err, records.ErrMetadataLimit) {
			t.Fatal(directory, err)
		}
	}
	if err := os.WriteFile(filepath.Join(client, "version.txt"), []byte("12.1.0.69876"), 0600); err != nil {
		t.Fatal(err)
	}
	_, err = records.OpenReader(store).ReadFile(context.Background(), *target.Pin.Data, records.FileQuery{Installation: client, FileDataID: 1, MetadataBytes: 1, ContentBytes: 1})
	if !errors.Is(err, records.ErrPinnedBuildChanged) {
		t.Fatal("changed client was accepted", err)
	}
}
