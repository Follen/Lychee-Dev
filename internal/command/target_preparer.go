package command

import (
	"context"
	"errors"
	"fmt"

	"github.com/follenfang/lycheedev/internal/records"
	"github.com/follenfang/lycheedev/internal/selection"
)

// recordsTargetPreparer is the narrow command-to-records adapter for mutable
// named target intent. Selection owns ordering/reference history; records owns
// installation and remote identity preparation.
type recordsTargetPreparer struct{}

func (recordsTargetPreparer) PrepareTarget(ctx context.Context, workspace string, config selection.TargetConfig, options selection.PrepareOptions) (selection.PinnedSet, error) {
	switch config.Source {
	case selection.TargetSourceInstallation:
		if config.FullBuild != "" {
			client, err := records.InspectClientInstallation(ctx, config.Installation)
			if err != nil {
				return selection.PinnedSet{}, err
			}
			if client.FullBuild != config.FullBuild {
				return selection.PinnedSet{}, fmt.Errorf("%w: installed %s, requested %s", selection.ErrTargetBuildUnavailable, client.FullBuild, config.FullBuild)
			}
		}
		reading, err := records.ResolveLocalTarget(ctx, workspace, records.LocalTargetRequest{Installation: config.Installation,
			Region: config.Region, Locale: config.Locale, Definitions: options.Definitions, Parent: options.Parent, Offline: options.Offline})
		return reading.Pin, err
	case selection.TargetSourceRemote:
		reading, err := records.ResolveRemoteTarget(ctx, workspace, records.RemoteTargetRequest{Product: config.Product,
			Region: config.Region, Locale: config.Locale, FullBuild: config.FullBuild, Definitions: options.Definitions,
			Parent: options.Parent, Offline: options.Offline})
		if config.FullBuild != "" && (errors.Is(err, records.ErrBuildUnavailable) || errors.Is(err, records.ErrRemoteObjectMissing)) {
			return selection.PinnedSet{}, fmt.Errorf("%w: %v", selection.ErrTargetBuildUnavailable, err)
		}
		return reading.Pin, err
	default:
		return selection.PinnedSet{}, selection.ErrTargetFormat
	}
}
