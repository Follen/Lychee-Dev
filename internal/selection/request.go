package selection

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"github.com/follenfang/lycheedev/internal/vault"
	"io"
	"os"
)

func ResolveSelectionFile(ctx context.Context, root, path string) (PinnedSet, error) {
	var spec SelectionSpec
	f, err := os.Open(path)
	if err != nil {
		return PinnedSet{}, err
	}
	defer f.Close()
	data, err := io.ReadAll(io.LimitReader(f, 65537))
	if err != nil {
		return PinnedSet{}, err
	}
	if len(data) > 65536 {
		return PinnedSet{}, errors.New("selection.spec_limit")
	}
	decoder := json.NewDecoder(bytes.NewReader(data))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&spec); err != nil {
		return PinnedSet{}, err
	}
	if err := decoder.Decode(new(any)); err != io.EOF {
		return PinnedSet{}, errors.New("selection.trailing_spec_data")
	}
	return vault.WriteMetadata(ctx, root, func(s *vault.Store, m *vault.Metadata) (PinnedSet, error) {
		return OpenPinner(m).PinSelection(ctx, spec)
	})
}

func InspectSelection(ctx context.Context, root, id string) (PinnedSet, error) {
	return vault.ReadWorkspace(ctx, root, func(s *vault.Store, m *vault.Metadata) (PinnedSet, error) {
		return OpenPinner(m).ReadPinnedSet(ctx, id)
	})
}
