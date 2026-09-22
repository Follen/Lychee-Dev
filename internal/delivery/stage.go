package delivery

import (
	"context"
	"errors"
	"fmt"
	"io"
	"os"
	"path"
	"path/filepath"
	"strings"
)

// StagePayload creates a verified private copy under stagingParent. It never
// changes an installed addon or skill. On success the caller owns the returned
// directory and must retain it until deployment finishes. On failure only this
// call's newly created directory is removed. Neither source nor parent is owned.
func StagePayload(ctx context.Context, source, stagingParent string, inventory []Resource) (staged string, err error) {
	// Do not retain the caller's mutable inventory while copying.
	inventory = append([]Resource(nil), inventory...)
	if err = VerifyPayload(ctx, source, inventory); err != nil {
		return "", err
	}
	parent, err := filepath.Abs(stagingParent)
	if err != nil {
		return "", err
	}
	parent, err = filepath.EvalSymlinks(parent)
	if err != nil {
		return "", err
	}
	directory, err := os.MkdirTemp(parent, ".lycheedev-stage-")
	if err != nil {
		return "", err
	}
	defer func() {
		if err != nil {
			// MkdirTemp guarantees a direct child; still validate before removal.
			if filepath.Dir(directory) != parent || !strings.HasPrefix(filepath.Base(directory), ".lycheedev-stage-") {
				err = errors.Join(err, fmt.Errorf("delivery.invalid_stage_path"))
				return
			}
			err = errors.Join(err, os.RemoveAll(directory))
		}
	}()
	input, err := os.OpenRoot(source)
	if err != nil {
		return "", err
	}
	defer input.Close()
	output, err := os.OpenRoot(directory)
	if err != nil {
		return "", err
	}
	defer output.Close()
	for _, entry := range inventory {
		if err = ctx.Err(); err != nil {
			return "", err
		}
		if err = output.MkdirAll(path.Dir(entry.Path), 0700); err != nil {
			return "", err
		}
		if err = copyResource(ctx, input, output, entry); err != nil {
			return "", err
		}
	}
	// Check copied bytes, not just the earlier source view. Any intervening
	// source change must match the manifest or the entire stage is discarded.
	if err = VerifyPayload(ctx, directory, inventory); err != nil {
		return "", err
	}
	return directory, nil
}

func copyResource(ctx context.Context, input, output *os.Root, entry Resource) (err error) {
	in, err := input.Open(entry.Path)
	if err != nil {
		return err
	}
	defer in.Close()
	info, err := in.Stat()
	if err != nil {
		return err
	}
	if !info.Mode().IsRegular() || info.Size() != entry.Bytes {
		return fmt.Errorf("%w: changed source %q", ErrPayload, entry.Path)
	}
	out, err := output.OpenFile(entry.Path, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
	if err != nil {
		return err
	}
	defer func() { err = errors.Join(err, out.Close()) }()
	n, err := io.Copy(out, &cancelReader{ctx: ctx, reader: io.LimitReader(in, entry.Bytes+1)})
	if err != nil {
		return err
	}
	if n != entry.Bytes {
		return fmt.Errorf("%w: changed source %q", ErrPayload, entry.Path)
	}
	return out.Sync()
}
