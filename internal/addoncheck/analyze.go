package addoncheck

import (
	"bytes"
	"context"
	"errors"
	"fmt"
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
	if len(data) > 32<<20 {
		return DocumentFacts{}, fmt.Errorf("codebase.analysis_byte_limit: %q has %d bytes, limit %d", name, len(data), 32<<20)
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
	// Lua source is byte-oriented: legacy bytes in comments or strings do not
	// invalidate declarations. Only the parser decides syntax completeness.
	result.Warnings = EncodingWarnings(data)
	return result, ctx.Err()
}

// EncodingWarnings locates the first invalid UTF-8 byte for display callers.
// It preserves the original bytes and makes no claim about document syntax.
func EncodingWarnings(data []byte) []EncodingWarning {
	if !utf8.Valid(data) {
		for offset := 0; offset < len(data); {
			r, width := utf8.DecodeRune(data[offset:])
			if r == utf8.RuneError && width == 1 {
				return []EncodingWarning{{
					Line:       bytes.Count(data[:offset], []byte{'\n'}) + 1,
					ByteOffset: offset,
					Message:    fmt.Sprintf("codebase.source_encoding: invalid UTF-8 byte 0x%02x at byte offset %d (zero-based); original bytes preserved; excerpts display invalid bytes as U+FFFD", data[offset], offset),
				}}
			}
			offset += width
		}
	}
	return nil
}
