package command

import (
	"context"
	"errors"

	"github.com/follenfang/lycheedev/internal/codebase"
	"github.com/follenfang/lycheedev/internal/luals"
)

// runSourceResearch composes the verified semantic runtime at the command
// boundary. The source engine can still return bounded structural evidence
// when a complete packaged runtime is unavailable on this machine.
func runSourceResearch(ctx context.Context, route string, opts Options, response *Envelope) (int, error) {
	if opts.snapshot == "" || (opts.symbolID == "") == (opts.symbol == "") {
		return 2, errors.New("source refs/context require --snapshot and exactly one of --symbol-id or --symbol")
	}
	if opts.staticOnly && opts.release != "" {
		return 2, errors.New("--static-only cannot be combined with --release")
	}
	root, err := workspaceRoot(opts.home)
	if err != nil {
		return 0, err
	}
	options := codebase.ResearchOptions{EnvironmentSnapshot: opts.environmentSnapshot, Flow: opts.sourceFlow}
	if !opts.staticOnly {
		options.Semantic, err = openLuaLS(ctx, opts.release)
		if err != nil {
			if opts.release != "" || !errors.Is(err, luals.ErrUnavailable) {
				return 0, err
			}
			response.Warnings = append(response.Warnings, "Verified LuaLS runtime unavailable; semantic coverage is incomplete: "+err.Error())
			options.Semantic = nil
		}
	}
	response.Context["snapshot"] = opts.snapshot
	switch route {
	case "source refs":
		reading, err := codebase.RelateSource(ctx, root, opts.snapshot, codebase.RelationQuery{
			SymbolID: opts.symbolID, Symbol: opts.symbol, Direction: opts.direction,
			Limit: opts.limit, Cursor: opts.cursor,
		}, options)
		if err != nil {
			return 0, err
		}
		response.Result = reading.Result
		response.Captures = append(response.Captures, reading.Capture)
	case "source context":
		reading, err := codebase.ContextSource(ctx, root, opts.snapshot, codebase.ContextQuery{
			SymbolID: opts.symbolID, Symbol: opts.symbol, Limit: opts.limit,
			MaxBytes: int(opts.maxBytes), MaxLines: opts.sourceMaxLines,
			Depth: opts.sourceDepth, Cursor: opts.cursor,
		}, options)
		if err != nil {
			return 0, err
		}
		response.Result = reading.Result
		response.Captures = append(response.Captures, reading.Capture)
	}
	return 0, nil
}
