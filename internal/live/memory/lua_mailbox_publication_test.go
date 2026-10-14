package memory

import (
	"bytes"
	"context"
	"encoding/binary"
	"errors"
	"fmt"
	"testing"

	"github.com/follenfang/lycheedev/internal/bridge"
)

func TestLuaMailboxChangedInputDiscardsSnapshotThenReadsNewPublication(t *testing.T) {
	f := newMailboxFixture(t, 0x140000000, 0x200000000)
	old := f.strings["input"]
	next := f.record("updated-input", bridge.MemoryInputState, [16]byte{})
	f.beforeRead = func(at uint64, _ int) {
		if at == old+32 {
			binary.LittleEndian.PutUint64(f.bytes(f.nodes["box.input"], 8), next)
		}
	}
	found, err := f.reader().Lookup(context.Background(), f.selectors["input"])
	if !errors.Is(err, ErrMailboxPathChanged) || !errors.Is(err, ErrMailboxUnavailable) || len(found.Records) != 0 || found.Coverage.Complete {
		t.Fatalf("changed publication authorized or lost its retry reason: %v %+v", err, found)
	}
	f.beforeRead = nil
	found, err = f.reader().Lookup(context.Background(), f.selectors["input"])
	if err != nil || len(found.Records) != 1 || !bytes.Equal(found.Records[0].Payload, f.payloads["updated-input"]) || f.regions != 0 {
		t.Fatalf("next read did not resolve the current publication: %v", err)
	}
}

func TestLuaMailboxRecipeGuardChangeIsNotPublicationRetry(t *testing.T) {
	f := newMailboxFixture(t, 0x140000000, 0x200000000)
	r := f.reader()
	r.readRoot = func(_ context.Context, _ uint64, b []byte) (int, error) { copy(b, []byte{2}); return len(b), nil }
	a := &luaAccess{reader: r}
	a.guard(r.root-16, []byte{1}, true)
	err := a.verify(context.Background())
	if !errors.Is(err, ErrMailboxUnavailable) || errors.Is(err, ErrMailboxPathChanged) || err.Error() != "memory.mailbox_unavailable: root_recipe_changed" {
		t.Fatalf("changed runtime instruction recipe was treated as temporary: %v", err)
	}
}

func TestLuaMailboxReplacedInvalidRecordReportsPathChange(t *testing.T) {
	for _, changed := range []bool{false, true} {
		for _, phase := range []string{"header", "payload", "layout"} {
			t.Run(fmt.Sprintf("changed=%t/%s", changed, phase), func(t *testing.T) {
				f := newMailboxFixture(t, 0x140000000, 0x200000000)
				old := f.strings["input"]
				next := f.record("replacement", bridge.MemoryInputState, [16]byte{})
				f.beforeRead = func(at uint64, _ int) {
					trigger := old
					if phase == "payload" {
						trigger += 32
					}
					if at != trigger {
						return
					}
					f.beforeRead = nil
					if changed {
						binary.LittleEndian.PutUint64(f.bytes(f.nodes["box.input"], 8), next)
					}
					if phase == "layout" {
						f.bytes(old+16, 1)[0] = 0
						return
					}
					for i, span := range f.spans {
						if span.address == old {
							f.spans = append(f.spans[:i], f.spans[i+1:]...)
							break
						}
					}
				}
				found, err := f.reader().Lookup(context.Background(), f.selectors["input"])
				if err == nil || len(found.Records) != 0 || errors.Is(err, ErrMailboxPathChanged) != changed {
					t.Fatalf("changed=%t returned %v %+v", changed, err, found)
				}
			})
		}
	}
}
