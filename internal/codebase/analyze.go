package codebase

import (
	"context"
	"github.com/follenfang/lycheedev/internal/addoncheck"
)

// AnalyzeDocument uses the same literal-only parser as installation checks.
func AnalyzeDocument(ctx context.Context, name string, data []byte) (DocumentFacts, error) {
	return addoncheck.AnalyzeDocument(ctx, name, data)
}
