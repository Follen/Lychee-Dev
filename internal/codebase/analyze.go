package codebase

import (
	"context"
	"errors"
	"path"
	"strings"
	"unicode/utf8"
)

// AnalyzeDocument returns syntax diagnostics as evidence, rather than hiding a
// malformed file or claiming that an incomplete extraction passed validation.
func AnalyzeDocument(ctx context.Context, name string, data []byte) (DocumentFacts, error) {
	if err := ctx.Err(); err != nil {
		return DocumentFacts{}, err
	}
	if len(data) > maxSourceBytes || !utf8.Valid(data) {
		return DocumentFacts{}, errors.New("codebase.analysis_encoding_or_budget")
	}
	var result DocumentFacts
	switch strings.ToLower(path.Ext(name)) {
	case ".lua":
		result = scanProgram(ctx, data, strings.Contains(name, "/Blizzard_APIDocumentationGenerated/"))
	case ".xml":
		result = scanMarkup(ctx, data)
	case ".toc":
		result = scanManifest(data)
	default:
		return DocumentFacts{}, errors.New("codebase.unsupported_document")
	}
	return result, ctx.Err()
}
