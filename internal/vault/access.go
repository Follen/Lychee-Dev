package vault

import (
	"context"
)

func WriteMetadata[T any](ctx context.Context, root string, run func(*Store, *Metadata) (T, error)) (T, error) {
	return accessWorkspace(ctx, root, false, run)
}

func ReadWorkspace[T any](ctx context.Context, root string, run func(*Store, *Metadata) (T, error)) (T, error) {
	return accessWorkspace(ctx, root, true, run)
}

func accessWorkspace[T any](ctx context.Context, root string, readOnly bool, run func(*Store, *Metadata) (T, error)) (T, error) {
	var zero T
	s, err := OpenStore(root)
	if err != nil {
		return zero, err
	}
	var m *Metadata
	if readOnly {
		m, err = s.ReadMetadata(ctx)
	} else {
		m, err = s.OpenMetadata(ctx)
	}
	if err != nil {
		return zero, err
	}
	defer m.Close()
	return run(s, m)
}
