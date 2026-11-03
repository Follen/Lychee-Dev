package records_test

import (
	"context"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/records"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
)

func TestLocalTargetsCurrentDeliveryProducts(t *testing.T) {
	for _, profile := range selection.DataProducts() {
		t.Run(profile.Product, func(t *testing.T) {
			fixture, client, store, _ := targetFixture(t, profile.ProductCode, profile.BuildSeries+".70338", profile.Folder)
			if err := os.Remove(filepath.Join(client, "version.txt")); err != nil {
				t.Fatal(err)
			}
			request := records.LocalTargetRequest{Installation: fixture.root, Product: profile.Product, Region: "cn", Locale: "zhCN", Definitions: strings.Repeat("d", 40), Offline: true}
			result, err := records.ResolveLocalTarget(context.Background(), store.Root(), request)
			if err != nil || result.Pin.Data == nil || result.Pin.Data.Product != profile.Product || result.Pin.Data.BuildConfig != fixture.buildKey || result.Client.Directory != client {
				t.Fatalf("root selection: %+v %v", result, err)
			}
			request.Installation, request.Product = client, ""
			exact, err := records.ResolveLocalTarget(context.Background(), store.Root(), request)
			if err != nil || exact.Pin.ID != result.Pin.ID {
				t.Fatalf("client selection changed identity: %+v %v", exact, err)
			}
		})
	}
}

func TestForeverSameBuildKeepsSeparatePinsAndRejectsSiblingReads(t *testing.T) {
	fixture, cn, store, _ := targetFixture(t, "wow_cn_beta", "1.60.1.70338", "_cn_beta_")
	asia := filepath.Join(fixture.root, "_classic_beta_")
	if err := os.Mkdir(asia, 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(asia, ".flavor.info"), []byte("wow_classic_beta"), 0600); err != nil {
		t.Fatal(err)
	}
	asiaConfig := validBuildConfig("wow_classic_beta")
	asiaKey := md5Key(asiaConfig)
	writeConfig(t, fixture.root, asiaKey, asiaConfig)
	writeCatalog(t, fixture.root, fmt.Sprintf("wow_cn_beta|%s|%s|%s|1\nwow_classic_beta|%s|%s|%s|1\n", fixture.fullBuild, fixture.buildKey, fixture.cdnKey, fixture.fullBuild, asiaKey, fixture.cdnKey))
	ctx := context.Background()
	pins := map[string]string{}
	for _, test := range []struct{ product, path, sibling, key string }{
		{"forever-cn-beta", cn, asia, fixture.buildKey},
		{"forever-asia-beta", asia, cn, asiaKey},
	} {
		if _, err := records.ResolveLocalTarget(ctx, store.Root(), records.LocalTargetRequest{Installation: fixture.root, Product: test.product, Region: "cn", Locale: "zhCN", Definitions: strings.Repeat("d", 40), Offline: true}); err == nil {
			t.Fatal("retired target accepted")
		}
		pin, err := vault.WriteMetadata(ctx, store.Root(), func(_ *vault.Store, m *vault.Metadata) (selection.PinnedSet, error) {
			return selection.OpenPinner(m).PinSelection(ctx, selection.SelectionSpec{Data: &selection.DataPin{Product: test.product, Region: "cn", Language: "zhCN", FullBuild: fixture.fullBuild, BuildConfig: test.key, CDNConfig: fixture.cdnKey, DefinitionCommit: strings.Repeat("d", 40)}})
		})
		if err != nil {
			t.Fatal(err)
		}
		pins[test.product] = pin.ID
		_, err = records.OpenReader(store).ReadFile(ctx, *pin.Data, records.FileQuery{Installation: test.sibling, FileDataID: 1, MetadataBytes: 1, ContentBytes: 1, Offline: true})
		if !errors.Is(err, records.ErrPinnedBuildChanged) {
			t.Fatalf("sibling read not rejected: %v", err)
		}
	}
	if pins["forever-cn-beta"] == pins["forever-asia-beta"] {
		t.Fatal("delivery identities collapsed")
	}
	if _, err := records.SelectClientInstallation(ctx, fixture.root, "forever"); !errors.Is(err, records.ErrInstallationMissing) {
		t.Fatalf("legacy selector guessed a region: %v", err)
	}
}
