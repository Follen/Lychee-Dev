package live

import (
	"context"
	"crypto/rand"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/vault"
	"path/filepath"
	"strings"
)

// PrepareProbe freezes a probe from a live verified session, retaining its
// evidence before reserving work. It neither edits the addon queue nor sends
// input. Admission is shared at the installation, not a machine input lease.
func (s *WindowSession) PrepareProbe(ctx context.Context, root, snapshot, account string, code []byte) (journal.WorkRecord, error) {
	return s.prepareProbe(ctx, root, snapshot, account, "", "", "cleaned", code, 0)
}

// PrepareProbeRevision freezes one immutable registry revision and a caller
// supplied idempotency key. Repeating the same request returns the original
// operation; changing any frozen input under that key is a conflict.
func (s *WindowSession) PrepareProbeRevision(ctx context.Context, root, snapshot, account, requestKey string, revision ProbeRevision) (journal.WorkRecord, error) {
	return s.PrepareProbeRevisionBudget(ctx, root, snapshot, account, requestKey, revision, 0)
}

// PrepareProbeRevisionBudget freezes the caller's execution limit with the
// immutable request. A resume uses this value and cannot grant a new run.
func (s *WindowSession) PrepareProbeRevisionBudget(ctx context.Context, root, snapshot, account, requestKey string, revision ProbeRevision, budgetSeconds int) (journal.WorkRecord, error) {
	if !validProbeRevision(revision) {
		return journal.WorkRecord{}, errors.New("live.probe_revision_corrupt")
	}
	if requestKey == "" || len(requestKey) > 128 {
		return journal.WorkRecord{}, errors.New("live.request_key_invalid")
	}
	return s.prepareProbe(ctx, root, snapshot, account, requestKey, revision.ID, "loaded", revision.Code, budgetSeconds)
}

func (s *WindowSession) prepareProbe(ctx context.Context, root, snapshot, account, requestKey, revision, goal string, code []byte, budgetSeconds int) (journal.WorkRecord, error) {
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
	if budgetSeconds < 0 || budgetSeconds > 120 {
		return zero, errors.New("live.execution_budget_invalid: supported range 1..120 seconds")
	}
	account, err := resolveReportAccount(ctx, s.target.Client.Directory, s.ready.Character, s.ready.Realm, account)
	if err != nil {
		return zero, err
	}
	if err := s.checkReportWriters(ctx); err != nil {
		return zero, err
	}
	var entropy [32]byte
	if _, err := rand.Read(entropy[:]); err != nil {
		return zero, err
	}
	ready := s.ready
	input := ReportIntent{
		Schema:        "lycheedev.report-intent.v1",
		Revision:      revision,
		BudgetSeconds: budgetSeconds,
		Expected:      bridge.SignalExpectation{Kind: "reported", Release: ready.Release, SessionNonce: ready.SessionNonce, RequestID: "REQ-" + hex.EncodeToString(entropy[:16]), Character: ready.Character, Realm: ready.Realm, Product: ready.Product, Build: ready.Build, AfterSequence: ready.Sequence},
		Code:          append([]byte(nil), code...),
		Load:          &ProbeLoadIntent{Installation: s.target.Client.Directory, Account: account, ReportScope: ready.ReportScope, GUID: ready.GUID, ReloadNonce: hex.EncodeToString(entropy[16:])},
	}
	raw, err := json.Marshal(input)
	if err != nil {
		return zero, err
	}
	intent := journal.WorkIntent{Kind: "probe", Resource: windowResource(s.target), Snapshot: snapshot, Session: ready.SessionNonce, Request: raw, Goal: goal}
	if requestKey != "" {
		completion := ""
		if goal == "finished" {
			completion = goal
		}
		identity := probeRequestIdentity(s.target, ready, snapshot, account, requestKey, revision, completion, code, budgetSeconds)
		intent.RequestKey, intent.RequestDigest = identity.RequestKey, identity.RequestDigest
	}
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

// One definition is shared by admission and pre-connect lookup. Refreshing
// readiness never changes a request's identity; changing the actor/build does.
func probeRequestIdentity(target ClientWindow, ready bridge.Signal, snapshot, account, requestKey, revision, completion string, code []byte, budgetSeconds int) journal.WorkIntent {
	resource := windowResource(target)
	identity, _ := json.Marshal(struct {
		Resource      string        `json:"resource"`
		Snapshot      string        `json:"snapshot"`
		Account       string        `json:"account"`
		Revision      string        `json:"revision"`
		SHA256        string        `json:"sha256"`
		BudgetSeconds int           `json:"budgetSeconds"`
		Completion    string        `json:"completion,omitempty"`
		Target        *ClientWindow `json:"target,omitempty"`
		GUID          string        `json:"guid,omitempty"`
	}{resource, snapshot, account, revision, fmt.Sprintf("%x", sha256.Sum256(code)), budgetSeconds, completion,
		func() *ClientWindow {
			if completion != "" {
				stable := target
				stable.Window.Title = ""
				stable.Client.IdentitySource = ""
				stable.Client.Directory = strings.ToLower(canonicalPath(stable.Client.Directory))
				stable.Window.Executable = strings.ToLower(canonicalPath(stable.Window.Executable))
				return &stable
			}
			return nil
		}(),
		func() string {
			if completion != "" {
				return ready.GUID
			}
			return ""
		}()})
	digest := sha256.Sum256(identity)
	key := sha256.Sum256([]byte(resource + "\x00" + requestKey))
	return journal.WorkIntent{RequestKey: "live-" + hex.EncodeToString(key[:]), RequestDigest: hex.EncodeToString(digest[:])}
}

func windowResource(target ClientWindow) string {
	return windowHandleResource(target.Window)
}

func windowHandleResource(window desktop.WindowIdentity) string {
	return fmt.Sprintf("window/%d/%d/%d", window.ProcessID, window.ProcessStartedAt, window.Handle)
}
