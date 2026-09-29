package delivery

import (
	"bytes"
	"context"
	"fmt"
	"path/filepath"
	"regexp"

	"github.com/follenfang/lycheedev/internal/bridge"
)

// NextFreeSlot is a read-only allocation hint under the publication lease.
// The caller journals the chosen index, then PublishSlot arbitrates any race.
func NextFreeSlot(pool SlotPool, after int) int {
	for i := after; i < len(pool.Files); i++ {
		s := pool.Files[i]
		if s.PendingHash == "" && (s.Nonce == "" || s.Consumed || s.RetiredRuntime != "" || s.RetiredProcess) {
			return i + 1
		}
	}
	return 0
}

var slotRuntimeLine = regexp.MustCompile(`(?m)^  runtime = "([0-9a-f]{32})",$`)
var slotActionLine = regexp.MustCompile(`(?m)^  action = "(bind|prepare|commit|confirm|release|unbind)",$`)

// VerifySlotRoute checks actual known payload bytes, not reservation ownership:
// a interrupted publication may still contain the previous payload. Call while
// holding the same publication lease as VerifySlotPublication and physical input.
// Skipping consumes only this client's LoD file; no foreign reservation is retired.
func VerifySlotRoute(ctx context.Context, parent string, e bridge.SlotEnvelope) error {
	start := e.StartSlot
	if start == 0 {
		start = e.Index
	}
	if start < 1 || start > e.Index || e.Index > bridge.SlotCount || (start != e.Index && e.Schema != bridge.SlotSchema) {
		return ErrInstallation
	}
	for i := start; i < e.Index; i++ {
		if err := ctx.Err(); err != nil {
			return err
		}
		b, err := readSlotFile(filepath.Join(SlotDirectory(parent, i), "Payload.lua"), 2<<20)
		if err != nil {
			return err
		}
		if bytes.Equal(b, []byte(inertSlot)) {
			continue
		}
		matches := slotRuntimeLine.FindAllSubmatch(b, 2)
		if len(matches) != 1 || string(matches[0][1]) == e.Runtime || !bytes.Contains(b, []byte("  schema = \""+bridge.SlotSchema+"\",\n")) || !bytes.Contains(b, []byte(fmt.Sprintf("  index = %d, fence = ", i))) || !slotActionLine.Match(b) {
			return fmt.Errorf("%w: slot route blocked at %d", ErrConflict, i)
		}
	}
	return nil
}
