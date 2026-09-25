package records

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"path/filepath"
	"time"

	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
)

var ErrRemoteUnavailable = errors.New("records.remote_unavailable_offline")
var ErrRemoteIdentity = errors.New("records.remote_identity")
var ErrRemoteHTTP = errors.New("records.remote_http")

type RemoteTargetRequest struct {
	Product, Region, Locale        string
	FullBuild, Definitions, Parent string
	Offline                        bool
}

func (r RemoteTargetRequest) Validate() error {
	if _, err := selection.DataProduct(r.Product); err != nil {
		return err
	}
	if _, err := selection.DataLocale(r.Region, r.Locale); err != nil {
		return err
	}
	if r.FullBuild != "" && !metadataBuild(r.FullBuild) {
		return ErrMetadataFormat
	}
	if ref, _ := normalizeDefinitionReference(r.Definitions); ref == "" {
		return ErrDefinitionIdentity
	}
	return nil
}

type RemoteTarget struct {
	Pin        selection.PinnedSet `json:"pin"`
	ObservedAt time.Time           `json:"observedAt"`
	Offline    bool                `json:"offline"`
	Capture    evidence.CaptureRef `json:"capture"`
}

// A remembered observation keeps both catalogs together. Offline reads must not
// combine a fresh versions row with an unrelated, partially refreshed CDN row.
type remoteObservation struct {
	ProductCode  string        `json:"productCode"`
	Region       string        `json:"region"`
	ObservedAt   time.Time     `json:"observedAt"`
	Versions     vault.BlobRef `json:"versions"`
	Distribution vault.BlobRef `json:"distribution"`
}

func ResolveRemoteTarget(ctx context.Context, root string, request RemoteTargetRequest) (RemoteTarget, error) {
	return resolveRemoteTarget(ctx, root, request, &http.Client{Timeout: 30 * time.Second})
}

func resolveRemoteTarget(ctx context.Context, root string, request RemoteTargetRequest, client *http.Client) (RemoteTarget, error) {
	if err := request.Validate(); err != nil {
		return RemoteTarget{}, err
	}
	ctx, cancel := context.WithTimeout(ctx, 2*time.Minute)
	defer cancel()
	return vault.WriteMetadata(ctx, root, func(store *vault.Store, metadata *vault.Metadata) (RemoteTarget, error) {
		var result RemoteTarget
		product, _ := selection.DataProduct(request.Product)
		slot, _ := selection.DataProductSlot(request.Product)
		pinner := selection.OpenPinner(metadata)
		definitions := request.Definitions
		if request.Parent != "" {
			parent, err := pinner.ReadPinnedSet(ctx, request.Parent)
			if err != nil {
				return result, err
			}
			if parent.Source != nil && parent.Source.Product != request.Product || parent.Data != nil && parent.Data.Product != request.Product || parent.Changes != nil && parent.Changes.Product != request.Product {
				return result, errors.New("selection.product_mismatch")
			}
			if parent.Data != nil {
				if definitions == "" {
					definitions = parent.Data.DefinitionCommit
				}
				if request.FullBuild == "" {
					request.FullBuild = parent.Data.FullBuild
				}
			}
		}
		catalogKey := "remote-catalog/" + product + "/" + request.Region
		var observation remoteObservation
		var versionsRaw, distributionRaw []byte
		var generation int64
		doc, err := metadata.ReadDocument(ctx, catalogKey)
		if err == nil {
			generation = doc.Generation
			if json.Unmarshal(doc.Value, &observation) != nil || observation.ProductCode != product || observation.Region != request.Region || observation.ObservedAt.IsZero() {
				return result, ErrRemoteIdentity
			}
		} else if !errors.Is(err, vault.ErrMissingRecord) {
			return result, err
		}
		base := remoteVersionBase(request.Region) + slot + "/"
		if request.Offline {
			if generation == 0 {
				return result, ErrRemoteUnavailable
			}
			versionsRaw, err = store.ReadBlob(ctx, observation.Versions, 1<<20)
			if err == nil {
				distributionRaw, err = store.ReadBlob(ctx, observation.Distribution, 1<<20)
			}
		} else {
			versionsRaw, err = fetchRemoteMetadata(ctx, client, base+"versions", 1<<20)
			if err == nil {
				distributionRaw, err = fetchRemoteMetadata(ctx, client, base+"cdns", 1<<20)
			}
			observation = remoteObservation{ProductCode: product, Region: request.Region, ObservedAt: time.Now().UTC()}
		}
		if err != nil {
			return result, err
		}
		release, err := parseReleaseCatalog(versionsRaw, request.Region, request.FullBuild)
		if err != nil {
			return result, err
		}
		route, err := parseDistributionCatalog(distributionRaw, request.Region)
		if err != nil {
			return result, err
		}
		buildDoc, buildRef, err := loadRemoteConfiguration(ctx, store, metadata, client, route, release.BuildConfig, request.Offline)
		if err != nil {
			return result, err
		}
		cdnDoc, cdnRef, err := loadRemoteConfiguration(ctx, store, metadata, client, route, release.CDNConfig, request.Offline)
		if err != nil {
			return result, err
		}
		_, err = completeBuildMetadata(BuildMetadata{Installed: InstalledBuild{Product: product, FullBuild: release.FullBuild, BuildConfig: release.BuildConfig, CDNConfig: release.CDNConfig}, BuildDocument: buildDoc, CDNDocument: cdnDoc})
		if err != nil {
			return result, err
		}
		defs := OpenDefinitions(store, metadata)
		defs.client = client
		commit, err := defs.ResolveRevision(ctx, definitions, request.Offline)
		if err != nil {
			return result, err
		}
		pin, err := pinner.PinSelection(ctx, selection.SelectionSpec{Parent: request.Parent, Data: &selection.DataPin{Product: request.Product, Region: request.Region, Language: request.Locale, FullBuild: release.FullBuild, BuildConfig: release.BuildConfig, CDNConfig: release.CDNConfig, DefinitionCommit: commit}})
		if err != nil {
			return result, err
		}
		if !request.Offline {
			observation.Versions, err = store.PublishBlob(ctx, vault.BlobInput{Reader: bytes.NewReader(versionsRaw), MaxBytes: 1 << 20})
			if err == nil {
				observation.Distribution, err = store.PublishBlob(ctx, vault.BlobInput{Reader: bytes.NewReader(distributionRaw), MaxBytes: 1 << 20})
			}
			if err != nil {
				return result, err
			}
			serialized, err := json.Marshal(observation)
			if err != nil {
				return result, err
			}
			err = metadata.CommitDocuments(ctx, vault.Mutation{Key: catalogKey, Value: serialized, ExpectedGeneration: generation})
			// Concurrent refreshes retain their own exact observations and pins.
			// The remembered alias is only a convenience for later offline use.
			if err != nil && !errors.Is(err, vault.ErrGeneration) {
				return result, err
			}
		}
		raw, err := json.Marshal(struct {
			Pin             selection.PinnedSet `json:"pin"`
			Observation     remoteObservation   `json:"observation"`
			VersionURL      string              `json:"versionURL"`
			DistributionURL string              `json:"distributionURL"`
			Route           distributionRoute   `json:"route"`
			Configurations  []vault.BlobRef     `json:"configurations"`
			Offline         bool                `json:"offline"`
		}{pin, observation, base + "versions", base + "cdns", route, []vault.BlobRef{buildRef, cdnRef}, request.Offline})
		if err != nil {
			return result, err
		}
		capture, err := evidence.OpenArchive(store, metadata).CommitCapture(ctx, evidence.CaptureDraft{Reader: bytes.NewReader(raw), MaxBytes: 65536, MediaType: "application/json", Complete: true, Provenance: evidence.Provenance{Kind: "remote-target", Locator: base + "versions", Snapshot: pin.ID, DataBuild: release.FullBuild}})
		if err != nil {
			return result, err
		}
		return RemoteTarget{Pin: pin, ObservedAt: observation.ObservedAt, Offline: request.Offline, Capture: capture}, nil
	})
}

func remoteVersionBase(region string) string {
	if region == "cn" {
		return "https://cn.version.battlenet.com.cn/"
	}
	return "https://" + region + ".version.battle.net/"
}

func fetchRemoteMetadata(ctx context.Context, client *http.Client, locator string, limit int64) ([]byte, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if client == nil {
		return nil, ErrRemoteIdentity
	}
	request, err := http.NewRequestWithContext(ctx, http.MethodGet, locator, nil)
	if err != nil {
		return nil, err
	}
	copy := *client
	copy.CheckRedirect = func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }
	response, err := copy.Do(request)
	if err != nil {
		return nil, fmt.Errorf("%w: %w", ErrRemoteHTTP, err)
	}
	defer response.Body.Close()
	if response.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("%w: status %d", ErrRemoteHTTP, response.StatusCode)
	}
	if response.ContentLength > limit {
		return nil, ErrMetadataLimit
	}
	raw, err := io.ReadAll(io.LimitReader(&metadataReader{ctx, response.Body}, limit+1))
	if err != nil {
		return nil, fmt.Errorf("%w: %w", ErrRemoteHTTP, err)
	}
	if int64(len(raw)) > limit {
		return nil, ErrMetadataLimit
	}
	return raw, nil
}

func loadRemoteConfiguration(ctx context.Context, store *vault.Store, metadata *vault.Metadata, client *http.Client, route distributionRoute, key string, offline bool) (ConfigDocument, vault.BlobRef, error) {
	cacheKey := "remote-config/" + key
	lease, err := vault.AcquireLease(ctx, filepath.Join(store.Root(), "locks"), cacheKey)
	if err != nil {
		return ConfigDocument{}, vault.BlobRef{}, err
	}
	defer lease.Close()
	doc, err := metadata.ReadDocument(ctx, cacheKey)
	if err == nil {
		var ref vault.BlobRef
		if json.Unmarshal(doc.Value, &ref) != nil {
			return ConfigDocument{}, ref, ErrRemoteIdentity
		}
		raw, err := store.ReadBlob(ctx, ref, 4<<20)
		if err != nil {
			return ConfigDocument{}, ref, err
		}
		parsed, err := parseConfiguration(raw, key)
		return parsed, ref, err
	}
	if !errors.Is(err, vault.ErrMissingRecord) {
		return ConfigDocument{}, vault.BlobRef{}, err
	}
	if offline {
		return ConfigDocument{}, vault.BlobRef{}, ErrRemoteUnavailable
	}
	var last error = ErrRemoteHTTP
	for _, host := range route.Hosts {
		locator := "https://" + host + "/" + route.Path + "/config/" + key[:2] + "/" + key[2:4] + "/" + key
		raw, err := fetchRemoteMetadata(ctx, client, locator, 4<<20)
		if err != nil {
			if ctx.Err() != nil {
				return ConfigDocument{}, vault.BlobRef{}, ctx.Err()
			}
			if !errors.Is(err, ErrRemoteHTTP) {
				return ConfigDocument{}, vault.BlobRef{}, err
			}
			last = err
			continue
		}
		parsed, err := parseConfiguration(raw, key)
		if err != nil {
			return ConfigDocument{}, vault.BlobRef{}, err
		}
		ref, err := store.PublishBlob(ctx, vault.BlobInput{Reader: bytes.NewReader(raw), MaxBytes: 4 << 20, ExpectedSHA256: parsed.SHA256})
		if err != nil {
			return ConfigDocument{}, vault.BlobRef{}, err
		}
		serialized, err := json.Marshal(ref)
		if err != nil {
			return ConfigDocument{}, vault.BlobRef{}, err
		}
		err = metadata.CommitDocuments(ctx, vault.Mutation{Key: cacheKey, Value: serialized})
		return parsed, ref, err
	}
	return ConfigDocument{}, vault.BlobRef{}, last
}
