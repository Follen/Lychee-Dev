package records

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"path/filepath"

	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
)

type LocalTargetRequest struct {
	Installation string
	Region       string
	Locale       string
	Definitions  string
	Parent       string
	Offline      bool
}

func (r LocalTargetRequest) Validate() error {
	if r.Installation == "" {
		return errors.New("records.target_installation_required")
	}
	_, err := selection.DataLocale(r.Region, r.Locale)
	return err
}

type LocalTarget struct {
	Pin          selection.PinnedSet          `json:"pin"`
	Client       selection.ClientInstallation `json:"client"`
	Installation string                       `json:"installation"`
	Capture      evidence.CaptureRef          `json:"capture"`
}

// ResolveLocalTarget prepares a fixed data identity from the selected client,
// never from folder-name guesses or another tool's state. Definition resolution
// happens once here; consumers subsequently use only the returned exact pin.
func ResolveLocalTarget(ctx context.Context, root string, request LocalTargetRequest) (LocalTarget, error) {
	if err := request.Validate(); err != nil {
		return LocalTarget{}, err
	}
	return vault.WriteMetadata(ctx, root, func(store *vault.Store, metadata *vault.Metadata) (LocalTarget, error) {
		var result LocalTarget
		client, err := InspectClientInstallation(ctx, request.Installation)
		if err != nil {
			return result, err
		}
		installation := filepath.Dir(client.Directory)
		build, err := ResolveLocalBuild(ctx, installation, client.ProductCode, client.FullBuild)
		if err != nil {
			return result, err
		}
		definitions := request.Definitions
		pinner := selection.OpenPinner(metadata)
		if request.Parent != "" {
			parent, err := pinner.ReadPinnedSet(ctx, request.Parent)
			if err != nil {
				return result, err
			}
			if parent.Source != nil && parent.Source.Product != client.Product || parent.Data != nil && parent.Data.Product != client.Product || parent.Changes != nil && parent.Changes.Product != client.Product {
				return result, errors.New("selection.product_mismatch")
			}
			if definitions == "" && parent.Data != nil {
				definitions = parent.Data.DefinitionCommit
			}
		}
		commit, err := OpenDefinitions(store, metadata).ResolveRevision(ctx, definitions, request.Offline)
		if err != nil {
			return result, err
		}
		// Downloads may outlive an installation update. Verify that the chosen
		// client and exact CASC row still agree before publishing its identity.
		current, err := InspectClientInstallation(ctx, request.Installation)
		if err != nil {
			return result, err
		}
		if current != client {
			return result, ErrPinnedBuildChanged
		}
		after, err := ResolveLocalBuild(ctx, installation, client.ProductCode, client.FullBuild)
		if err != nil {
			return result, err
		}
		if after.Installed != build.Installed {
			return result, ErrPinnedBuildChanged
		}
		build = after
		pin, err := pinner.PinSelection(ctx, selection.SelectionSpec{Parent: request.Parent, Data: &selection.DataPin{
			Product: client.Product, Region: request.Region, Language: request.Locale, FullBuild: client.FullBuild,
			BuildConfig: build.Installed.BuildConfig, CDNConfig: build.Installed.CDNConfig, DefinitionCommit: commit,
		}})
		if err != nil {
			return result, err
		}
		var configs []vault.BlobRef
		for _, doc := range []ConfigDocument{build.BuildDocument, build.CDNDocument} {
			ref, err := store.PublishBlob(ctx, vault.BlobInput{Reader: bytes.NewReader(doc.Raw), MaxBytes: 4 << 20, ExpectedSHA256: doc.SHA256})
			if err != nil {
				return result, err
			}
			configs = append(configs, ref)
		}
		raw, err := json.Marshal(struct {
			Pin            selection.PinnedSet          `json:"pin"`
			Client         selection.ClientInstallation `json:"client"`
			Installation   string                       `json:"installation"`
			CatalogSHA256  string                       `json:"catalogSHA256"`
			Configurations []vault.BlobRef              `json:"configurations"`
		}{pin, client, installation, build.CatalogSHA256, configs})
		if err != nil {
			return result, err
		}
		capture, err := evidence.OpenArchive(store, metadata).CommitCapture(ctx, evidence.CaptureDraft{
			Reader: bytes.NewReader(raw), MaxBytes: 65536, MediaType: "application/json", Complete: true,
			Provenance: evidence.Provenance{Kind: "local-target", Locator: client.Directory, Snapshot: pin.ID, DataBuild: client.FullBuild},
		})
		if err != nil {
			return result, err
		}
		return LocalTarget{Pin: pin, Client: client, Installation: installation, Capture: capture}, nil
	})
}
