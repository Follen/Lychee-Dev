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
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/vault"
	"path/filepath"
	"time"
)

// Clean retires the exact disk queue and requests one cleanup reload. It leaves
// the operation acknowledged until fresh game and disk evidence prove completion.
// Partial submission is unresolved, never permission to resend the command.
func (p *ProbeOperation) Clean(ctx context.Context) (desktop.InputReceipt, error) {
	return p.clean(ctx, desktop.QueuePreparedCommand)
}

func (p *ProbeOperation) clean(ctx context.Context, send preparedInput) (desktop.InputReceipt, error) {
	if err := p.check(ctx); err != nil {
		return desktop.InputReceipt{}, err
	}
	record, err := journal.OpenBook(p.metadata).InspectWork(ctx, p.id)
	if err != nil {
		return desktop.InputReceipt{}, err
	}
	var observed reportObservation
	if err := json.Unmarshal(record.Observation, &observed); err != nil {
		return desktop.InputReceipt{}, err
	}
	if record.Stage != "acknowledged" || record.Status != "running" || observed.CleanupReadyID != "" {
		return desktop.InputReceipt{}, journal.ErrTransition
	}
	if send == nil {
		return desktop.InputReceipt{}, errors.New("live.input_sender_missing")
	}
	if _, err := p.RetireQueue(ctx); err != nil {
		return desktop.InputReceipt{}, err
	}
	return p.submitInput(ctx, "acknowledged", send, p.prepareCleanupInput)
}

func (p *ProbeOperation) prepareCleanupInput(ctx context.Context) (string, error) {
	return withOperation(ctx, p.root, p.id, func(store *vault.Store, metadata *vault.Metadata) (string, error) {
		if err := p.check(ctx); err != nil {
			return "", err
		}
		book := journal.OpenBook(metadata)
		record, err := book.InspectWork(ctx, p.id)
		if err != nil {
			return "", err
		}
		var observed reportObservation
		if err := json.Unmarshal(record.Observation, &observed); err != nil {
			return "", err
		}
		if record.Stage != "acknowledged" || record.Status != "running" || observed.CleanupReadyID != "" {
			return "", journal.ErrTransition
		}
		input, definition, err := probeDefinition(record)
		if err != nil {
			return "", err
		}
		deployment, _, err := delivery.ResolveAddonDestination(ctx, input.Load.Installation)
		if err != nil {
			return "", err
		}
		if deployment.Client.Product != input.Expected.Product || deployment.Client.FullBuild != input.Expected.Build {
			return "", errors.New("live.probe_installation_mismatch")
		}
		archive := evidence.OpenArchive(store, metadata)
		if _, err := acknowledgedReport(ctx, archive, record, input, observed); err != nil {
			return "", err
		}
		if observed.QueueRetirement == nil || observed.QueueRetirement.Revision.SHA256 == "" {
			return "", errors.New("live.cleanup_prerequisite_missing")
		}
		if _, err := delivery.VerifyProbeRetired(ctx, filepath.Join(p.session.target.Client.Directory, "Interface", "AddOns", "Lychee Dev"), definition); err != nil {
			return "", err
		}
		_, rawAck, err := archive.FetchCapture(ctx, observed.AcknowledgementID, 4096)
		if err != nil {
			return "", err
		}
		ack, err := bridge.ParseSignal(rawAck)
		if err != nil {
			return "", err
		}
		if p.session.ready.RuntimeEpoch == 0 || p.session.ready.RuntimeEpoch >= 9007199254740991 {
			return "", errors.New("live.reload_epoch_unavailable")
		}
		expected := input.Expected
		expected.Kind, expected.RequestID, expected.ReloadNonce = "ready", "", ""
		expected.RuntimeEpoch, expected.AfterSequence, expected.RequireInputReady = p.session.ready.RuntimeEpoch, ack.Sequence, true
		wait, cancel := context.WithTimeout(ctx, 15*time.Second)
		defer cancel()
		signal, err := p.session.reader.WaitForSignal(wait, expected)
		if err != nil {
			return "", err
		}
		if err := validateCleanupReadiness(signal, input, p.session.ready.RuntimeEpoch, ack.Sequence); err != nil {
			return "", err
		}
		if err := p.check(ctx); err != nil {
			return "", err
		}
		raw, _ := json.Marshal(signal)
		capture, err := archive.CommitCapture(ctx, evidence.CaptureDraft{Reader: bytes.NewReader(raw), MaxBytes: 4096, MediaType: "application/json", Complete: true, Provenance: evidence.Provenance{Kind: "decoded-cleanup-readiness", Locator: input.Expected.RequestID, Snapshot: record.Intent.Snapshot, OperationID: p.id, DataBuild: input.Expected.Build, Session: input.Expected.SessionNonce}})
		if err != nil {
			return "", err
		}
		if observed.CleanupNonce == "" {
			var nonce [16]byte
			if _, err := rand.Read(nonce[:]); err != nil {
				return "", err
			}
			observed.CleanupNonce = hex.EncodeToString(nonce[:])
		}
		if decoded, err := hex.DecodeString(observed.CleanupNonce); err != nil || len(decoded) != 16 || hex.EncodeToString(decoded) != observed.CleanupNonce {
			return "", errors.New("live.invalid_cleanup_nonce")
		}
		observed.CleanupReadyID = capture.ID
		raw, _ = json.Marshal(observed)
		if _, err := delivery.VerifyProbeRetired(ctx, filepath.Join(p.session.target.Client.Directory, "Interface", "AddOns", "Lychee Dev"), definition); err != nil {
			return "", err
		}
		if err := p.check(ctx); err != nil {
			return "", err
		}
		if err := book.AdvanceStage(ctx, journal.StageChange{OperationID: p.id, ExpectedGeneration: record.Generation, ExpectedStage: "acknowledged", Stage: "acknowledged", Status: "running", Observation: raw}); err != nil {
			return "", err
		}
		return "/dev bridge clean " + input.Expected.RequestID + " " + observed.CleanupNonce, nil
	})
}
