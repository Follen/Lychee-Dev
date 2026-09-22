package codebase

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
)

type AddonAssessment struct {
	Result  CompatibilityAssessment
	Capture evidence.CaptureRef
}

func AssessAddon(ctx context.Context, root, snapshot string, input AddonInput) (AddonAssessment, error) {
	return vault.WriteMetadata(ctx, root, func(s *vault.Store, m *vault.Metadata) (AddonAssessment, error) {
		var result AddonAssessment
		pin, err := selection.OpenPinner(m).ReadPinnedSet(ctx, snapshot)
		if err != nil {
			return result, err
		}
		if pin.Source == nil {
			return result, errors.New("codebase.source_pin_required")
		}
		result.Result, err = OpenChecker(s).CheckClosure(ctx, *pin.Source, input)
		if err != nil {
			return result, err
		}
		raw, err := json.Marshal(result.Result)
		if err != nil {
			return result, err
		}
		result.Capture, err = evidence.OpenArchive(s, m).CommitCapture(ctx, evidence.CaptureDraft{Reader: bytes.NewReader(raw), MaxBytes: 16 << 20, MediaType: "application/json", Complete: result.Result.Complete,
			Provenance: evidence.Provenance{Kind: "addon-static-check", Locator: input.Root + "/" + input.Manifest, Snapshot: snapshot, SourceCommit: pin.Source.ExactCommit},
		})
		return result, err
	})
}
