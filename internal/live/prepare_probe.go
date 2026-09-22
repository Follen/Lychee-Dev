package live

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/vault"
	"path/filepath"
)

// PrepareProbe freezes a probe from a live verified session, retaining its
// evidence before reserving work. It neither edits the addon queue nor sends
// input. Admission is shared at the installation, not a machine input lease.
func (s *WindowSession) PrepareProbe(ctx context.Context, root, snapshot, account string, code []byte) (journal.WorkRecord, error) {
	var zero journal.WorkRecord
	if s == nil || s.closed || s.reader == nil || s.confirm == nil {
		return zero, errors.New("live.session_closed")
	}
	if err := ctx.Err(); err != nil {
		return zero, err
	}
	if len(code) == 0 || len(code) > 256<<10 {
		return zero, errors.New("bridge.queue_invalid_code")
	}
	account, err := resolveReportAccount(ctx, s.target.Client.Directory, s.ready.Character, s.ready.Realm, account)
	if err != nil {
		return zero, err
	}
	var entropy [32]byte
	if _, err := rand.Read(entropy[:]); err != nil {
		return zero, err
	}
	ready := s.ready
	input := ReportIntent{
		Schema:   "lycheedev.report-intent.v1",
		Expected: bridge.SignalExpectation{Kind: "reported", Release: ready.Release, SessionNonce: ready.SessionNonce, RequestID: "REQ-" + hex.EncodeToString(entropy[:16]), Character: ready.Character, Realm: ready.Realm, Product: ready.Product, Build: ready.Build, AfterSequence: ready.Sequence},
		Code:     append([]byte(nil), code...),
		Load:     &ProbeLoadIntent{Installation: s.target.Client.Directory, Account: account, GUID: ready.GUID, ReloadNonce: hex.EncodeToString(entropy[16:])},
	}
	raw, err := json.Marshal(input)
	if err != nil {
		return zero, err
	}
	intent := journal.WorkIntent{Kind: "probe", Resource: windowResource(s.target), Snapshot: snapshot, Session: ready.SessionNonce, Request: raw}
	if _, _, err := probeDefinition(journal.WorkRecord{Intent: intent}); err != nil {
		return zero, err
	}
	binding, err := SaveWindowSession(ctx, root, snapshot, s)
	if err != nil {
		return zero, err
	}
	input.Binding = binding.ID
	intent.Request, err = json.Marshal(input)
	if err != nil {
		return zero, err
	}
	return vault.WriteMetadata(ctx, root, func(store *vault.Store, metadata *vault.Metadata) (journal.WorkRecord, error) {
		if _, err := readSessionEvidence(ctx, store, metadata, binding); err != nil {
			return zero, err
		}
		if err := s.confirm(ctx, s.target); err != nil {
			return zero, err
		}
		return journal.OpenBook(metadata).BeginWindowWork(ctx, filepath.Join(s.target.Client.Directory, "Interface", "AddOns"), store.Identity().WorkspaceID, intent)
	})
}

func windowResource(target ClientWindow) string {
	return windowHandleResource(target.Window)
}

func windowHandleResource(window desktop.WindowIdentity) string {
	return fmt.Sprintf("window/%d/%d/%d", window.ProcessID, window.ProcessStartedAt, window.Handle)
}
