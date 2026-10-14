package memory

import (
	"context"
	"encoding/binary"
	"errors"
	"fmt"
	"strings"
	"testing"
)

// A real Lua-path traversal loses an object after its parent was read. Only a
// changed prefix can classify the resulting short read as a publication gap.
func TestMailboxTraversalRetirementRequiresChangedPath(t *testing.T) {
	for _, point := range []string{"state_root", "globals_value", "namespace_value", "mailbox_value", "hash_nodes"} {
		for _, changed := range []bool{false, true} {
			t.Run(fmt.Sprintf("%s/changed=%t", point, changed), func(t *testing.T) {
				f := newMailboxFixture(t, 0x140000000, 0x200000000)
				oldRoot, oldState := f.rootBytes, f.state
				oldGlobals, oldNamespace := f.tables["globals"], f.nodes["globals.LycheeDevInternal"]
				oldMailbox := f.nodes["namespace.Mailbox"]
				var trigger uint64
				switch point {
				case "state_root":
					trigger = oldState + 16
				case "globals_value":
					trigger = oldGlobals
				case "namespace_value":
					trigger = f.tables["namespace"]
				case "mailbox_value":
					trigger = f.tables["box"]
				case "hash_nodes":
					trigger = oldNamespace
				}
				f.beforeRead = func(at uint64, _ int) {
					if at != trigger {
						return
					}
					f.beforeRead = nil
					if changed {
						f.install(0x300000000)
						if point != "state_root" {
							f.rootBytes = oldRoot
						}
						switch point {
						case "globals_value":
							binary.LittleEndian.PutUint64(f.bytes(oldState+0x90, 8), f.tables["globals"])
						case "namespace_value":
							binary.LittleEndian.PutUint64(f.bytes(oldNamespace, 8), f.tables["namespace"])
						case "mailbox_value":
							binary.LittleEndian.PutUint64(f.bytes(oldMailbox, 8), f.tables["box"])
						case "hash_nodes":
							copy(f.bytes(oldGlobals+40, 8), f.bytes(f.tables["globals"]+40, 8))
						}
					}
					for i := 0; i < len(f.spans); {
						if trigger >= f.spans[i].address && trigger < f.spans[i].address+uint64(len(f.spans[i].data)) {
							f.spans = append(f.spans[:i], f.spans[i+1:]...)
						} else {
							i++
						}
					}
				}
				found, err := f.reader().Lookup(context.Background(), f.selectors["input"])
				if err == nil || len(found.Records) != 0 || found.Coverage.Complete || errors.Is(err, ErrMailboxPathChanged) != changed {
					t.Fatalf("changed=%t expected exact path-change classification, got err=%v records=%d complete=%t", changed, err, len(found.Records), found.Coverage.Complete)
				}
				if !changed && !strings.Contains(err.Error(), "short_read") {
					t.Fatalf("stable path hid original read failure: %v", err)
				}
				if changed {
					found, err = f.reader().Lookup(context.Background(), f.selectors["input"])
					if err != nil || len(found.Records) != 1 || f.regions != 0 {
						t.Fatalf("new named path was not readable: err=%v records=%d", err, len(found.Records))
					}
				}
			})
		}
	}
}

func TestMailboxTraversalReplacementDoesNotHideProcessOrCancellation(t *testing.T) {
	for _, failure := range []string{"process_identity", "cancelled"} {
		t.Run(failure, func(t *testing.T) {
			f := newMailboxFixture(t, 0x140000000, 0x200000000)
			state := f.state
			ctx, cancel := context.WithCancel(context.Background())
			defer cancel()
			processErr := errors.New("fixture.process_changed")
			f.beforeRead = func(at uint64, _ int) {
				if at != state+16 {
					return
				}
				f.beforeRead = nil
				f.install(0x300000000)
				if failure == "cancelled" {
					cancel()
				} else {
					f.verifyError = func(int) error { return processErr }
				}
				for i, span := range f.spans {
					if span.address == state {
						f.spans = append(f.spans[:i], f.spans[i+1:]...)
						break
					}
				}
			}
			found, err := f.reader().Lookup(ctx, f.selectors["input"])
			want := processErr
			if failure == "cancelled" {
				want = context.Canceled
			}
			if !errors.Is(err, want) || errors.Is(err, ErrMailboxPathChanged) || len(found.Records) != 0 || found.Coverage.Complete {
				t.Fatalf("independent failure became a retry: err=%v records=%d complete=%t", err, len(found.Records), found.Coverage.Complete)
			}
		})
	}
}
