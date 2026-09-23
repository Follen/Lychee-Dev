package live

import (
	"bytes"
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/delivery"
	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/vault"
	"path/filepath"
	"time"
)

func (p *ProbeOperation) cleanupInput(ctx context.Context) (journal.WorkRecord, ReportIntent, reportObservation, error) {
	var input ReportIntent
	var observed reportObservation
	if err := p.check(ctx); err != nil {
		return journal.WorkRecord{}, input, observed, err
	}
	record, err := journal.OpenBook(p.metadata).InspectWork(ctx, p.id)
	if err != nil {
		return record, input, observed, err
	}
	if record.Stage != "acknowledged" || record.Status != "running" && record.Status != "unresolved" {
		return record, input, observed, journal.ErrTransition
	}
	input, definition, err := probeDefinition(record)
	if err != nil {
		return record, input, observed, err
	}
	if err := json.Unmarshal(record.Observation, &observed); err != nil {
		return record, input, observed, err
	}
	if observed.CleanupReadyID != "" && observed.ClearedID == "" {
		return record, input, observed, errors.New("live.cleanup_runtime_unobserved")
	}
	if observed.Schema != "lycheedev.report-observation.v1" || observed.RemovalID == "" || observed.QueueRetirement == nil || observed.QueueRetirement.Revision.SHA256 == "" {
		return record, input, observed, errors.New("live.cleanup_prerequisite_missing")
	}
	_, err = delivery.VerifyProbeRetired(ctx, delivery.AddonDirectory(p.session.target.Client.Directory), definition)
	return record, input, observed, err
}

// PrepareCompletion records one random verification challenge before returning
// the read-only addon command. It never sends input; callers must reload/rebind
// as needed and satisfy current input eligibility themselves. A retry preserves
// the challenge and cannot be mistaken for a completed operation.
func (p *ProbeOperation) PrepareCompletion(ctx context.Context) (string, error) {
	record, input, observed, err := p.cleanupInput(ctx)
	if err != nil {
		return "", err
	}
	if observed.CleanupNonce == "" {
		var nonce [16]byte
		if _, err := rand.Read(nonce[:]); err != nil {
			return "", err
		}
		observed.CleanupNonce = hex.EncodeToString(nonce[:])
		if err := p.check(ctx); err != nil {
			return "", err
		}
		raw, _ := json.Marshal(observed)
		if err := journal.OpenBook(p.metadata).AdvanceStage(ctx, journal.StageChange{OperationID: p.id, ExpectedGeneration: record.Generation, ExpectedStage: "acknowledged", Stage: "acknowledged", Status: "running", Observation: raw}); err != nil {
			return "", err
		}
	}
	if decoded, err := hex.DecodeString(observed.CleanupNonce); err != nil || len(decoded) != 16 || hex.EncodeToString(decoded) != observed.CleanupNonce {
		return "", errors.New("live.invalid_cleanup_nonce")
	}
	return "/dev bridge verify " + input.Expected.RequestID + " " + observed.CleanupNonce, nil
}

// Complete observes a fresh selected-window challenge response and rechecks disk
// absence and queue retirement before committing cleaned. It then drops its OS
// lease and retires only its durable owner. If retirement fails, cleaned remains
// recorded; the caller must reconcile that marker, never replay the probe.
func (p *ProbeOperation) Complete(ctx context.Context) (journal.WorkRecord, error) {
	if p == nil || p.run == nil || p.metadata == nil {
		return journal.WorkRecord{}, errors.New("live.operation_closed")
	}
	initial, err := p.run.Check(ctx)
	if err != nil {
		return journal.WorkRecord{}, err
	}
	var pending reportObservation
	if err := json.Unmarshal(initial.Observation, &pending); err != nil {
		return journal.WorkRecord{}, err
	}
	var signal bridge.Signal
	if pending.CleanupReadyID != "" {
		signal, err = p.observeCleanupReentry(ctx)
		if err != nil {
			return journal.WorkRecord{}, err
		}
		if _, err := p.ObserveRemoval(ctx); err != nil {
			return journal.WorkRecord{}, err
		}
	}
	record, input, observed, err := p.cleanupInput(ctx)
	if err != nil {
		return journal.WorkRecord{}, err
	}
	if observed.CleanupNonce == "" {
		return journal.WorkRecord{}, errors.New("live.cleanup_challenge_missing")
	}
	if pending.CleanupReadyID == "" {
		if p.session.ready.RuntimeEpoch == 0 {
			return journal.WorkRecord{}, errors.New("live.reload_epoch_unavailable")
		}
		expected := input.Expected
		expected.Kind, expected.CleanupNonce, expected.ReloadNonce = "cleared", observed.CleanupNonce, ""
		expected.AfterSequence, expected.RequireInputReady = 0, false
		expected.RuntimeEpoch = p.session.ready.RuntimeEpoch
		wait, cancel := context.WithTimeout(ctx, 15*time.Second)
		defer cancel()
		signal, err = p.session.waitOperationSignal(wait, input.Load.Installation, expected)
		if err != nil {
			return journal.WorkRecord{}, err
		}
	}
	if signal.GUID != input.Load.GUID {
		return journal.WorkRecord{}, errors.New("live.cleanup_guid_mismatch")
	}
	if err := p.check(ctx); err != nil {
		return journal.WorkRecord{}, err
	}
	if _, err := p.ObserveRemoval(ctx); err != nil {
		return journal.WorkRecord{}, err
	}
	record, input, observed, err = p.cleanupInput(ctx)
	if err != nil {
		return journal.WorkRecord{}, err
	}
	if signal.CleanupNonce != observed.CleanupNonce {
		return journal.WorkRecord{}, errors.New("live.cleanup_challenge_changed")
	}
	store, err := vault.OpenStore(p.root)
	if err != nil {
		return journal.WorkRecord{}, err
	}
	raw, _ := json.Marshal(signal)
	capture, err := evidence.OpenArchive(store, p.metadata).CommitCapture(ctx, evidence.CaptureDraft{Reader: bytes.NewReader(raw), MaxBytes: 4096, MediaType: "application/json", Complete: true, Provenance: evidence.Provenance{Kind: "decoded-game-cleared", Locator: input.Expected.RequestID, Snapshot: record.Intent.Snapshot, OperationID: p.id, DataBuild: input.Expected.Build, Session: input.Expected.SessionNonce}})
	if err != nil {
		return journal.WorkRecord{}, err
	}
	if err := p.check(ctx); err != nil {
		return journal.WorkRecord{}, err
	}
	observed.ClearedID = capture.ID
	raw, _ = json.Marshal(observed)
	book := journal.OpenBook(p.metadata)
	if err := book.AdvanceStage(ctx, journal.StageChange{OperationID: p.id, ExpectedGeneration: record.Generation, ExpectedStage: "acknowledged", Stage: "cleaned", Status: "completed", Observation: raw}); err != nil {
		return journal.WorkRecord{}, err
	}
	completed, err := book.InspectWork(ctx, p.id)
	if err != nil {
		return completed, err
	}
	if err := p.run.Close(); err != nil {
		return completed, err
	}
	p.run = nil
	return ReleaseCompletedProbe(ctx, p.root, p.id)
}

// ReleaseCompletedProbe reconciles only a terminal probe's durable owner after
// completion/lease-release interruption. It verifies retained completion evidence
// without attaching to a game or sending input. A different owner's marker is
// never removed; an absent marker is already released.
func ReleaseCompletedProbe(ctx context.Context, root, operationID string) (journal.WorkRecord, error) {
	return withOperation(ctx, root, operationID, func(store *vault.Store, metadata *vault.Metadata) (journal.WorkRecord, error) {
		book := journal.OpenBook(metadata)
		record, err := book.InspectWork(ctx, operationID)
		if err != nil {
			return record, err
		}
		if record.Stage != "cleaned" || record.Status != "completed" {
			return record, journal.ErrTransition
		}
		input, _, err := probeDefinition(record)
		if err != nil {
			return record, err
		}
		var observed reportObservation
		if err := json.Unmarshal(record.Observation, &observed); err != nil {
			return record, err
		}
		if observed.Schema != "lycheedev.report-observation.v1" || observed.CleanupNonce == "" || observed.ClearedID == "" || observed.QueueRetirement == nil || observed.QueueRetirement.Revision.SHA256 == "" {
			return record, errors.New("live.cleanup_evidence_missing")
		}
		binding, err := ReadWindowSession(ctx, store.Root(), input.Binding)
		if err != nil {
			return record, err
		}
		if binding.Record.Snapshot != record.Intent.Snapshot || windowResource(binding.Target) != record.Intent.Resource || binding.Ready.GUID != input.Load.GUID || binding.Ready.SessionNonce != input.Expected.SessionNonce {
			return record, errors.New("live.operation_binding_mismatch")
		}
		ref, raw, err := evidence.OpenArchive(store, metadata).FetchCapture(ctx, observed.ClearedID, 4096)
		if err != nil {
			return record, err
		}
		want := evidence.Provenance{Kind: "decoded-game-cleared", Locator: input.Expected.RequestID, Snapshot: record.Intent.Snapshot, OperationID: operationID, DataBuild: input.Expected.Build, Session: input.Expected.SessionNonce}
		if ref.Provenance != want || !ref.Complete || ref.Truncated || ref.MediaType != "application/json" {
			return record, errors.New("live.cleanup_provenance_mismatch")
		}
		signal, err := bridge.ParseSignal(raw)
		if err != nil {
			return record, err
		}
		expected := input.Expected
		expected.Kind, expected.CleanupNonce, expected.ReloadNonce = "cleared", observed.CleanupNonce, ""
		expected.AfterSequence, expected.RequireInputReady = 0, false
		if observed.CleanupReadyID != "" {
			base, err := executionBase(ctx, store.Root(), record, input, binding)
			if err != nil {
				return record, err
			}
			expected, err = cleanupExpectation(input, base, observed)
			if err != nil {
				return record, err
			}
		} else {
			ready := binding.Ready
			if observed.ReloadedCapture != "" {
				ready, _, err = readReloadEvidence(ctx, store.Root(), record, input, binding, observed.ReloadedCapture)
				if err != nil {
					return record, err
				}
			}
			if ready.RuntimeEpoch == 0 {
				return record, errors.New("live.reload_epoch_unavailable")
			}
			expected.RuntimeEpoch = ready.RuntimeEpoch
		}
		if err := signal.Match(expected); err != nil {
			return record, err
		}
		if signal.GUID != input.Load.GUID {
			return record, errors.New("live.cleanup_guid_mismatch")
		}
		if observed.CleanupReadyID != "" {
			if _, _, err := readCleanupEvidence(ctx, store.Root(), record, input, binding, observed); err != nil {
				return record, err
			}
		}
		err = book.RetireWindowWork(ctx, filepath.Join(binding.Target.Client.Directory, "Interface", "AddOns"), store.Identity().WorkspaceID, operationID)
		return record, err
	})
}
