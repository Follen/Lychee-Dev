package codebase

import (
	"context"
	"fmt"
	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/vault"
)

type AddonAssessment struct {
	Result  CompatibilityAssessment
	Capture evidence.CaptureRef
}

func AssessAddon(ctx context.Context, root, snapshot string, input AddonInput, options ...ValidationOptions) (AddonAssessment, error) {
	var result AddonAssessment
	pin, err := pinnedSource(ctx, root, snapshot)
	if err != nil {
		return result, err
	}
	s, err := vault.OpenStore(root)
	if err != nil {
		return result, err
	}
	b := OpenBrowser(s)
	checkPin := pin
	if pin.Repository != "wow-ui-source" && (len(options) == 0 || options[0].EnvironmentSnapshot == "") {
		return result, fmt.Errorf("%w: codebase.client_environment_required", ErrInvalidQuery)
	}
	if len(options) > 0 && options[0].EnvironmentSnapshot != "" {
		checkPin, _, err = researchEnvironmentPin(ctx, root, pin, options[0].EnvironmentSnapshot)
		if err != nil {
			return result, err
		}
	}
	if err = b.EnsureIndex(ctx, checkPin); err != nil {
		return result, err
	}
	result.Result, err = OpenChecker(s).CheckClosure(ctx, checkPin, input)
	if err != nil {
		return result, err
	}
	result.Result.Source = pin
	if checkPin != pin {
		result.Result.Environment = &checkPin
	}
	if len(options) > 0 && options[0].Semantic != nil {
		result.Result.Semantic, err = b.checkSemantic(ctx, pin, result.Result.Load, options[0].Semantic, options[0].EnvironmentSnapshot)
		if err != nil {
			return result, err
		}
		result.Result.StaticValid = result.Result.StaticValid && result.Result.Semantic.Passed
		result.Result.Complete = false // LuaLS does not establish runtime safety or full WoW API coverage.
		result.Result.Load.CompatibilityComplete = false
		result.Result.Checks = append(result.Result.Checks, "luals-frozen-closure")
	}
	result.Capture, err = captureSourceJSON(ctx, root, result.Result, result.Result.Complete, false, evidence.Provenance{Kind: "addon-static-check", Locator: input.Root + "/" + input.Manifest, Snapshot: snapshot, SourceCommit: pin.ExactCommit})
	return result, err
}
