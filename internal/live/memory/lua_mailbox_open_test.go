package memory

import (
	"bytes"
	"context"
	"encoding/binary"
	"errors"
	"strings"
	"testing"
)

func TestLuaMailboxRecipeToCurrentPublicationAcrossRelocation(t *testing.T) {
	image := newRootRecipeFixture(t, 0x2000, 4096, 0x4000)
	image.base = 0x7ff600000000
	anchor, root := image.textRVA+128, image.dataRVA+16
	image.anchor(t, anchor, root)
	heap := newMailboxFixture(t, image.base, 0x300000000)
	binary.LittleEndian.PutUint64(image.data[root-image.dataRVA:], heap.state)
	layout, err := readLuaRootImageLayout(bytes.NewReader(image.header), rootRecipeUnknownHash(), image.size)
	if err != nil {
		t.Fatal(err)
	}
	reader, err := OpenLuaMailbox(context.Background(), heap, image.base, image.size, rootRecipeUnknownHash(), "fixture-release", layout, image.read)
	if err != nil || reader.Binding().RootRVA != root || reader.Binding().Method != "recipe_unique_text" {
		t.Fatal("new executable did not derive its own root", err)
	}
	for i, base := range []uint64{0x300000000, 0x900000000} {
		if i > 0 {
			heap.install(base)
			binary.LittleEndian.PutUint64(image.data[root-image.dataRVA:], heap.state)
		}
		result, err := reader.Lookup(context.Background(), heap.selectors["input"])
		if err != nil || len(result.Records) != 1 || result.Records[0].Address != heap.strings["input"]+32 || heap.regions != 0 {
			t.Fatal("derived root did not resolve current heap publication", result, err)
		}
	}
	// A new code binding invalidates an already open reader; retained old root
	// and record allocations cannot authorize continuing with the earlier recipe.
	image.text[anchor-image.textRVA+3] ^= 8
	result, err := reader.Lookup(context.Background(), heap.selectors["input"])
	if !errors.Is(err, ErrMailboxUnavailable) || !strings.Contains(err.Error(), "root_recipe_changed") || len(result.Records) != 0 {
		t.Fatal("changed recipe kept the old root authorized", err)
	}
}
