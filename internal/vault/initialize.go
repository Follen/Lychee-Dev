package vault

import (
	"context"
)

type WorkspaceSummary struct {
	Root     string   `json:"root"`
	Identity Identity `json:"identity"`
}

func InitializeWorkspace(ctx context.Context, root string) (WorkspaceSummary, error) {
	store, err := Initialize(ctx, root)
	if err != nil {
		return WorkspaceSummary{}, err
	}
	return WorkspaceSummary{Root: store.Root(), Identity: store.Identity()}, nil
}
