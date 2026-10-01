package codebase

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"github.com/follenfang/lycheedev/internal/codebase/environment"
	"github.com/follenfang/lycheedev/internal/luals"
	"github.com/follenfang/lycheedev/internal/selection"
)

// SemanticAnalyzer is supplied only by composition. The adapter acquires and
// retains its own verified lease; the caller has not acquired one underneath it.
type SemanticAnalyzer func(context.Context, selection.SourcePin, string, environment.Manifest, []luals.Query) (luals.Analysis, error)

// SemanticWorkspace contains only independently verified inputs. A broker
// cannot supply a runtime executable, checkout path, definitions or shell text.
type SemanticWorkspace struct {
	Lease       *WorktreeLease
	Definitions []byte
	Identity    string
}

func (b *Browser) VerifySemanticWorkspace(ctx context.Context, pin selection.SourcePin, environmentSnapshot, recordsHash string, expected environment.Manifest, current *WorktreeLease) (result SemanticWorkspace, err error) {
	ctx, cleanup := sourceQueryContext(ctx)
	defer cleanup()
	if err = b.EnsureIndex(ctx, pin); err != nil {
		return result, err
	}
	cache, _, err := b.openIndex(ctx, pin)
	if err != nil {
		return result, err
	}
	defer cache.Close()
	if cache.manifest.RecordsHash != recordsHash {
		return result, ErrSourceIntegrity
	}
	env, _, err := b.researchEnvironment(ctx, pin, environmentSnapshot)
	if err != nil {
		return result, err
	}
	got, _ := json.Marshal(env.Manifest)
	want, _ := json.Marshal(expected)
	if string(got) != string(want) {
		return result, fmt.Errorf("%w: semantic environment changed", ErrSourceIntegrity)
	}
	if current == nil {
		current, err = b.AcquireWorktree(ctx, pin)
		if err != nil {
			return result, err
		}
		defer func() {
			if err != nil {
				current.Close()
			}
		}()
	}
	if current.Path() != b.worktreePath(pin) {
		return result, ErrSourceIntegrity
	}
	manifest, err := b.verifyWorktree(ctx, pin, current.Path())
	if err != nil {
		return result, err
	}
	mappings, err := json.Marshal(env.Mappings)
	if err != nil {
		return result, err
	}
	mappingHash := sha256.Sum256(mappings)
	config, _ := luals.ConfigurationIdentity()
	key, err := json.Marshal([]any{"lycheedev.semantic-workspace.v1", pin, manifest, recordsHash, env.Manifest, hex.EncodeToString(mappingHash[:]), config, "128-query/relative-uri/v2"})
	if err != nil {
		return result, err
	}
	digest := sha256.Sum256(key)
	result = SemanticWorkspace{Lease: current, Definitions: env.Definitions, Identity: hex.EncodeToString(digest[:])}
	return result, nil
}
