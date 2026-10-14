package codebase

import (
	"context"
	"errors"
	"github.com/follenfang/lycheedev/internal/addoncheck"
	"github.com/follenfang/lycheedev/internal/vault"
)

type AddonInput = addoncheck.AddonInput
type ValidationIssue = addoncheck.ValidationIssue
type LoadStep = addoncheck.LoadStep
type LoadedDocument = addoncheck.LoadedDocument
type LoadAssessment = addoncheck.LoadAssessment

type Checker struct{ store *vault.Store }

func OpenChecker(store *vault.Store) *Checker { return &Checker{store: store} }

func (c *Checker) InspectAddon(ctx context.Context, input AddonInput) (LoadAssessment, error) {
	if c.store == nil {
		return LoadAssessment{}, errors.New("codebase.archive_required")
	}
	return addoncheck.InspectLoad(ctx, input, c.store, maxSourceBytes)
}

func InspectLocalLoad(ctx context.Context, input AddonInput) (LoadAssessment, error) {
	return addoncheck.InspectLoad(ctx, input, nil, maxSourceBytes)
}
