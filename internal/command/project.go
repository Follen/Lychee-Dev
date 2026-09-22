package command

import (
	"context"
	"errors"
	"os"

	"github.com/follenfang/lycheedev/internal/selection"
)

func selectProject(ctx context.Context, opts *Options) (selection.ProjectStatus, error) {
	directory, err := projectDirectory(opts.project)
	if opts.project == "" && errors.Is(err, selection.ErrProjectMissing) {
		return selection.ProjectStatus{}, nil
	}
	if err != nil {
		return selection.ProjectStatus{}, err
	}
	root, err := workspaceRoot(opts.home)
	if err != nil {
		return selection.ProjectStatus{}, err
	}
	status, err := selection.LoadProjectSelection(ctx, root, directory)
	if err != nil {
		return selection.ProjectStatus{}, err
	}
	opts.snapshot = status.Lock.Selection.ID
	return status, nil
}

func projectDirectory(explicit string) (string, error) {
	if explicit != "" {
		return explicit, nil
	}
	cwd, err := os.Getwd()
	if err != nil {
		return "", err
	}
	return selection.FindProject(cwd)
}
