package selection

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"github.com/follenfang/lycheedev/internal/vault"
	"io"
	"os"
)

var (
	ErrSpecFormat = errors.New("selection.spec_format")
	ErrSpecLimit  = errors.New("selection.spec_limit")
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
		return PinnedSet{}, ErrSpecLimit
	}
	decoder := json.NewDecoder(bytes.NewReader(data))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&spec); err != nil {
		return PinnedSet{}, fmt.Errorf("%w: %w", ErrSpecFormat, err)
	}
	if err := decoder.Decode(new(any)); err != io.EOF {
		return PinnedSet{}, fmt.Errorf("%w: trailing selection data", ErrSpecFormat)
	}
	return vault.WriteMetadata(ctx, root, func(s *vault.Store, m *vault.Metadata) (PinnedSet, error) {
		return OpenPinner(m).PinCurrentSelection(ctx, spec)
	})
}

func InspectSelection(ctx context.Context, root, id string) (PinnedSet, error) {
	return vault.ReadWorkspace(ctx, root, func(s *vault.Store, m *vault.Metadata) (PinnedSet, error) {
		return OpenPinner(m).ReadPinnedSet(ctx, id)
	})
}
