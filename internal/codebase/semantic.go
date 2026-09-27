package codebase

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"sort"
	"strings"

	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/luals"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
)

type ValidationOptions struct {
	Semantic            *luals.Runtime
	EnvironmentSnapshot string
}

type SemanticAssessment struct {
	luals.Result
	Definitions DefinitionCoverage             `json:"definitions"`
	Inputs      []SemanticInput                `json:"inputs"`
	Artifacts   map[string]vault.BlobRef       `json:"artifacts"`
	Captures    map[string]evidence.CaptureRef `json:"captures"`
	NotChecked  []string                       `json:"notChecked"`
}
type SemanticInput struct {
	Path    string               `json:"path"`
	Blob    vault.BlobRef        `json:"blob"`
	Capture *evidence.CaptureRef `json:"capture,omitempty"`
}
type DefinitionCoverage struct {
	State         string          `json:"state"`
	Documents     int             `json:"documents"`
	Functions     int             `json:"functions"`
	Skipped       int             `json:"skipped"`
	SkippedSample []string        `json:"skippedSample"`
	Truncated     bool            `json:"truncated"`
	Sources       []SemanticInput `json:"sources"`
}

func (b *Browser) checkSemantic(ctx context.Context, pin selection.SourcePin, load LoadAssessment, engine *luals.Runtime, environmentSnapshot string) (*SemanticAssessment, error) {
	definitions, coverage, err := b.semanticDefinitions(ctx, pin, environmentSnapshot)
	if err != nil {
		return nil, err
	}
	result := &SemanticAssessment{Definitions: coverage, Inputs: []SemanticInput{}, Artifacts: map[string]vault.BlobRef{}, Captures: map[string]evidence.CaptureRef{}, NotChecked: []string{
		"unknown or dynamic API metadata and client-specific globals", "XML inline scripts and injected frame globals",
		"external addon dependencies", "TOC execution order and addon varargs identity", "dynamic code", "combat, taint, secret values and runtime behavior",
	}}
	files := map[string][]byte{}
	for _, doc := range load.Documents {
		if !strings.HasSuffix(strings.ToLower(doc.Path), ".lua") {
			continue
		}
		data, err := b.store.ReadBlob(ctx, doc.Blob, luals.MaxFileBytes)
		if err != nil {
			return nil, err
		}
		// LuaLS requires a .lua extension, but evidence retains the source path.
		name := strings.TrimSuffix(doc.Path, doc.Path[len(doc.Path)-4:]) + ".lua"
		files[name] = data
		result.Inputs = append(result.Inputs, SemanticInput{Path: doc.Path, Blob: doc.Blob})
	}
	result.Result, err = engine.Check(ctx, files, definitions)
	if err != nil {
		return nil, err
	}
	for i := range result.Diagnostics {
		for _, input := range result.Inputs {
			if strings.EqualFold(input.Path, result.Diagnostics[i].Path) {
				result.Diagnostics[i].Path = input.Path
				break
			}
		}
	}
	// Register every dependent byte stream as its own capture. A main
	// assessment that only contains blob digests would leave them outside the
	// evidence archive's retention graph.
	err = b.registerSemanticEvidence(ctx, pin, result, files, definitions)
	return result, err
}

func (b *Browser) registerSemanticEvidence(ctx context.Context, pin selection.SourcePin, result *SemanticAssessment, files map[string][]byte, definitions []byte) error {
	_, err := vault.WriteMetadata(ctx, b.store.Root(), func(s *vault.Store, m *vault.Metadata) (struct{}, error) {
		archive := evidence.OpenArchive(s, m)
		for i := range result.Inputs {
			input := &result.Inputs[i]
			data := files[strings.TrimSuffix(input.Path, input.Path[len(input.Path)-4:])+".lua"]
			capture, err := archive.CommitCapture(ctx, evidence.CaptureDraft{Reader: bytes.NewReader(data), MaxBytes: luals.MaxFileBytes, MediaType: "text/x-lua", Complete: true, Provenance: evidence.Provenance{Kind: "source-semantic-input", Locator: input.Path, SourceCommit: pin.ExactCommit}})
			if err != nil {
				return struct{}{}, err
			}
			input.Capture = &capture
		}
		for _, artifact := range []struct {
			name  string
			data  []byte
			media string
		}{
			{"definitions", definitions, "text/x-lua"}, {"configuration", result.Config, "application/json"}, {"rawReport", result.Raw, "application/json"},
		} {
			capture, err := archive.CommitCapture(ctx, evidence.CaptureDraft{Reader: bytes.NewReader(artifact.data), MaxBytes: luals.MaxReportBytes, MediaType: artifact.media, Complete: true, Provenance: evidence.Provenance{Kind: "source-semantic-artifact", Locator: artifact.name, SourceCommit: pin.ExactCommit}})
			if err != nil {
				return struct{}{}, err
			}
			result.Captures[artifact.name] = capture
			result.Artifacts[artifact.name] = capture.Blob
		}
		return struct{}{}, nil
	})
	return err
}

// semanticDefinitions uses the same pinned generated API environment as
// source relations and flow. It does not create a second primitive-only view.
func (b *Browser) semanticDefinitions(ctx context.Context, pin selection.SourcePin, environmentSnapshot string) ([]byte, DefinitionCoverage, error) {
	coverage := DefinitionCoverage{State: "unavailable", SkippedSample: []string{}, Sources: []SemanticInput{}}
	parsed, state, err := b.researchEnvironment(ctx, pin, environmentSnapshot)
	if err != nil {
		return nil, coverage, err
	}
	if state != "ready" {
		return nil, coverage, fmt.Errorf("%w: API environment %s", luals.ErrUnavailable, state)
	}
	coverage.State = "complete"
	if !parsed.Coverage.Complete {
		coverage.State = "partial"
	}
	coverage.Documents = parsed.Coverage.Documents
	for _, mapping := range parsed.Mappings {
		if mapping.Kind == "function" {
			coverage.Functions++
		}
	}
	coverage.Skipped = len(parsed.Coverage.Gaps)
	coverage.Truncated = parsed.Coverage.Truncated
	for _, gap := range parsed.Coverage.Gaps {
		if len(coverage.SkippedSample) >= 200 {
			coverage.Truncated = true
			break
		}
		coverage.SkippedSample = append(coverage.SkippedSample, fmt.Sprintf("%s:%d: %s", gap.Path, gap.Line, gap.Code))
	}
	// Stable diagnostics make it possible to compare the exact environment
	// used by a semantic check against source research of the same pin.
	sort.Strings(coverage.SkippedSample)
	if len(parsed.Definitions) == 0 {
		return nil, coverage, errors.New("codebase.empty_api_environment")
	}
	return parsed.Definitions, coverage, nil
}

// EnsureIndex prepares only the exact already-pinned commit, never a branch
// head. It avoids an extra agent turn between synchronization and research.
func (b *Browser) EnsureIndex(ctx context.Context, pin selection.SourcePin) error {
	db, _, err := b.openIndex(ctx, pin)
	if err == nil {
		db.Close()
		return nil
	}
	if !errors.Is(err, errIndexNotReady) {
		return err
	}
	_, err = b.IndexSource(ctx, pin)
	return err
}
