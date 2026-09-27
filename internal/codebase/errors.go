package codebase

import "errors"

var (
	ErrInvalidSearchCursor = errors.New("codebase.invalid_search_cursor")
	ErrInvalidSymbol       = errors.New("codebase.invalid_symbol")
	ErrAmbiguousSymbol     = errors.New("codebase.ambiguous_symbol")
	ErrSourceNotReady      = errors.New("codebase.source_not_ready")
	ErrSourceBudget        = errors.New("codebase.source_budget")
	ErrSourceIntegrity     = errors.New("codebase.source_integrity")
	ErrUnknownProduct      = errors.New("codebase.unknown_product")
	ErrUnknownRepository   = errors.New("codebase.repository_not_found")
	ErrInvalidSourceRef    = errors.New("codebase.invalid_source_ref")
)
