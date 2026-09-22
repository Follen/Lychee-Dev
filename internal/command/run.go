package command

import (
	"context"
	"errors"
	"github.com/follenfang/lycheedev/internal/live"
	"io"
	"os"
)

func runLiveProbe(ctx context.Context, opts Options) (live.Outcome, error, int) {
	var zero live.Outcome
	request := live.RunRequest{Session: opts.session, Account: opts.account}
	if opts.file == "" || opts.session == "" {
		return zero, errors.New("live run requires --session and --file; --account is optional when the character directory uniquely matches"), 2
	}
	file, err := os.Open(opts.file)
	if err != nil {
		return zero, err, 2
	}
	defer file.Close()
	info, err := file.Stat()
	if err != nil {
		return zero, err, 2
	}
	if !info.Mode().IsRegular() || info.Size() > 256<<10 {
		return zero, errors.New("probe must be a regular file of at most 256 KiB"), 2
	}
	request.Code, err = io.ReadAll(io.LimitReader(file, (256<<10)+1))
	if err != nil {
		return zero, err, 2
	}
	if err := request.Validate(); err != nil {
		return zero, err, 2
	}
	root, err := workspaceRoot(opts.home)
	if err != nil {
		return zero, err, 0
	}
	record, err := live.Run(ctx, root, request)
	return record, err, 0
}
