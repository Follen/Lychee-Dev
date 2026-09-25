package command

import (
	"context"
	"errors"
	"io"
	"os"

	"github.com/follenfang/lycheedev/internal/live"
)

func readProbeFile(path string) ([]byte, error) {
	if path == "" {
		return nil, errors.New("probe file is required")
	}
	file, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer file.Close()
	info, err := file.Stat()
	if err != nil {
		return nil, err
	}
	if !info.Mode().IsRegular() || info.Size() < 1 || info.Size() > 256<<10 {
		return nil, errors.New("probe must be a non-empty regular file of at most 256 KiB")
	}
	return io.ReadAll(io.LimitReader(file, (256<<10)+1))
}

func putLiveProbe(ctx context.Context, opts Options) (live.ProbePutResult, error, int) {
	if opts.name == "" || opts.file == "" {
		return live.ProbePutResult{}, errors.New("live probe put requires --name and --file"), 2
	}
	code, err := readProbeFile(opts.file)
	if err != nil {
		return live.ProbePutResult{}, err, 2
	}
	root, err := workspaceRoot(opts.home)
	if err != nil {
		return live.ProbePutResult{}, err, 0
	}
	result, err := live.PutProbe(ctx, root, opts.name, code)
	return result, err, 0
}
