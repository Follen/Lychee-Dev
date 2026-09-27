package command

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"os"
	"path/filepath"

	"github.com/follenfang/lycheedev/internal/delivery"
	"github.com/follenfang/lycheedev/internal/live"
	"github.com/follenfang/lycheedev/internal/selection"
)

func updateToolkit(ctx context.Context, opts Options) (any, error, int) {
	root, err := workspaceRoot(opts.home)
	if err != nil {
		return nil, err, 3
	}
	state := root + ".delivery"
	release := opts.release
	if release == "" {
		executable, err := os.Executable()
		if err != nil {
			return nil, err, 3
		}
		release = filepath.Clean(filepath.Join(filepath.Dir(executable), "..", ".."))
		if !opts.plan {
			return nil, errors.New("update requires the npm launcher for self-update; native archive users pass --release <installed-release-root>"), 3
		}
	}
	targets := []delivery.UpdateTarget{}
	if opts.file != "" {
		if len(opts.updatePaths)+len(opts.updateInstallations) != 0 {
			return nil, errors.New("update --file cannot be combined with target selectors"), 2
		}
		file, err := os.Open(opts.file)
		if err != nil {
			return nil, err, 3
		}
		raw, readErr := io.ReadAll(io.LimitReader(file, (64<<10)+1))
		closeErr := file.Close()
		if err := errors.Join(readErr, closeErr); err != nil {
			return nil, err, 3
		}
		if len(raw) > 64<<10 {
			return nil, errors.New("update target file exceeds 64 KiB"), 2
		}
		decoder := json.NewDecoder(bytes.NewReader(raw))
		decoder.DisallowUnknownFields()
		if err := decoder.Decode(&targets); err != nil {
			return nil, err, 2
		}
		if targets == nil {
			return nil, errors.New("update targets must be a JSON array"), 2
		}
		if err := decoder.Decode(new(any)); err != io.EOF {
			return nil, errors.New("update target file has trailing data"), 2
		}
	}
	for _, path := range opts.updatePaths {
		targets = append(targets, delivery.UpdateTarget{Component: "skill", Path: path})
	}
	for _, path := range opts.updateInstallations {
		targets = append(targets, delivery.UpdateTarget{Component: "addon", Path: path})
	}
	if len(targets) == 0 && opts.file == "" {
		targets, err = delivery.ReadUpdateTargets(state)
		if err != nil {
			return nil, err, 4
		}
		user, err := userHomeDirectory()
		if err != nil {
			return nil, err, 3
		}
		codex := os.Getenv("CODEX_HOME")
		if codex == "" {
			codex = filepath.Join(user, ".codex")
		}
		for _, path := range []string{filepath.Join(user, ".agents", "skills", "lycheedev"), filepath.Join(codex, "skills", "lycheedev")} {
			if _, err := os.Lstat(path); err == nil {
				targets = append(targets, delivery.UpdateTarget{Component: "skill", Path: path})
			} else if !errors.Is(err, os.ErrNotExist) {
				return nil, err, 3
			}
		}
		// Passive discovery never captures, wakes, reconnects or types into games.
		roots := []string{}
		for _, target := range targets {
			if target.Component == "addon" {
				roots = append(roots, target.Path, filepath.Dir(target.Path))
			}
		}
		if _, err := os.Stat(filepath.Join(root, "workspace.json")); err == nil {
			configured, err := selection.ListTargets(ctx, root)
			if err != nil {
				return nil, err, 3
			}
			for _, target := range configured {
				if target.Source == selection.TargetSourceInstallation {
					roots = append(roots, target.Installation)
				}
			}
		}
		discovered, err := live.DiscoverCandidates(ctx, root, live.DiscoveryRequest{Passive: true, Roots: roots})
		if err != nil {
			return nil, err, 3
		}
		for _, item := range discovered.Installations {
			path := item.Client.Directory
			status, err := delivery.InspectDeployment(ctx, delivery.AddonDirectory(path), "addon")
			if err != nil {
				return nil, err, 3
			}
			if status.State != "absent" {
				targets = append(targets, delivery.UpdateTarget{Component: "addon", Path: path})
			}
		}
	}
	result, err := delivery.UpdateManaged(ctx, release, Version, state, targets, opts.plan)
	return result, err, 0
}
