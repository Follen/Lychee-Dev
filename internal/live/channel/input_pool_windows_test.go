//go:build windows && amd64

package channel

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/desktop"
)

func TestNativeReloadDoesNotRequireSlotPoolButInvokeStillDoes(t *testing.T) {
	for _, damage := range []string{"missing", "corrupt"} {
		for _, kind := range []string{"reload", "invoke"} {
			t.Run(damage+"/"+kind, func(t *testing.T) {
				n := &Native{Parent: t.TempDir(), Version: "2.5.1", Consumer: "123/456", Guard: func(context.Context) error { return nil }}
				e := bridge.SlotEnvelope{}
				marker := filepath.Join(n.Parent, ".lycheedev-slots.json")
				if damage == "corrupt" {
					if err := os.WriteFile(marker, []byte("broken"), 0600); err != nil {
						t.Fatal(err)
					}
				}
				// An invalid observation stops before desktop input. Supplying a
				// monitor avoids creating a real keyboard hook in this disk test.
				n.keyboardActivity = &desktop.KeyboardActivity{}
				ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
				defer cancel()
				out, err := n.Input(ctx, InputAction{Kind: kind, Capability: InputSchema, Envelope: e})
				if out.MessagesQueued != 0 || out.Disposition != "not_sent" || n.Publication != nil {
					t.Fatalf("unexpected input/lease: %+v %v", out, err)
				}
				if kind == "reload" {
					if !errors.Is(err, ErrPending) || out.Reason != "input_bindings_invalid" {
						t.Fatalf("reload did not reach fresh INPUT gate: %+v %v", out, err)
					}
				} else if err == nil || out.Reason != "slot_publication_changed" {
					t.Fatalf("invoke skipped publication gate: %+v %v", out, err)
				}
			})
		}
	}
}
