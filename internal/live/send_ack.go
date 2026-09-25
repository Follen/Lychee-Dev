package live

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"github.com/follenfang/lycheedev/internal/delivery"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/vault"
	"io"
	"time"
	// Acknowledge sends one short command only after archiving/verifying the report
	// and observing current input readiness. No queue rewrite or reload is involved.
	// Input submission is not acknowledgement; Observe must confirm the receipt.
)

// Missing fresh readiness is a recoverable observation boundary, not a
// cancelled probe. The verified report and ownership remain available.
var ErrAckReadinessPending = errors.New("live.ack_readiness_pending")

func (p *ProbeOperation) Acknowledge(ctx context.Context) (desktop.InputReceipt, error) {
	return p.acknowledge(ctx, desktop.QueuePreparedCommand)
}

func (p *ProbeOperation) acknowledge(ctx context.Context, send preparedInput) (desktop.InputReceipt, error) {
	if err := p.check(ctx); err != nil {
		return desktop.InputReceipt{}, err
	}
	record, err := journal.OpenBook(p.metadata).InspectWork(ctx, p.id)
	if err != nil {
		return desktop.InputReceipt{}, err
	}
	if record.Stage != "verified" || record.Status != "running" {
		return desktop.InputReceipt{}, journal.ErrTransition
	}
	return p.submitInput(ctx, "ack_requested", send, func(ctx context.Context) (string, error) {
		if err := p.prepareAcknowledgement(ctx); err != nil {
			return "", err
		}
		report, err := RequestReportAcknowledgement(ctx, p.root, p.id)
		if err != nil {
			return "", err
		}
		return fmt.Sprintf("/dev bridge ack %s %d", report.Receipt.RequestID, report.Receipt.Sequence), nil
	})
}

func (p *ProbeOperation) prepareAcknowledgement(ctx context.Context) error {
	if err := p.check(ctx); err != nil {
		return err
	}
	_, err := withOperation(ctx, p.root, p.id, func(store *vault.Store, metadata *vault.Metadata) (bool, error) {
		book := journal.OpenBook(metadata)
		record, err := book.InspectWork(ctx, p.id)
		if err != nil {
			return false, err
		}
		if record.Stage != "verified" || record.Status != "running" {
			return false, journal.ErrTransition
		}
		input, _, err := probeDefinition(record)
		if err != nil {
			return false, err
		}
		var observed reportObservation
		if err := json.Unmarshal(record.Observation, &observed); err != nil {
			return false, err
		}
		if observed.Schema != "lycheedev.report-observation.v1" {
			return false, errors.New("live.invalid_report_observation")
		}
		archive := evidence.OpenArchive(store, metadata)
		if _, err := archive.ReadVerifiedReport(ctx, observed.BodyID, observed.ReceiptID, input.Code, input.Expected, p.id, record.Intent.Snapshot); err != nil {
			return false, err
		}
		deployment, _, err := delivery.ResolveAddonDestination(ctx, input.Load.Installation)
		if err != nil {
			return false, err
		}
		if deployment.Client.Product != input.Expected.Product || deployment.Client.FullBuild != input.Expected.Build {
			return false, errors.New("live.probe_installation_mismatch")
		}
		expected := input.Expected
		expected.Kind, expected.RequestID, expected.ReloadNonce = "ready", "", ""
		expected.RuntimeEpoch, expected.RequireInputReady = p.session.ready.RuntimeEpoch, true
		expected.AfterSequence = p.session.ready.Sequence - 1
		if observed.ReloadedCapture == "" {
			if input.Revision == "" {
				return false, errors.New("live.reload_not_observed")
			}
			binding, err := ReadWindowSession(ctx, p.root, input.Binding)
			if err != nil {
				return false, err
			}
			base, err := executionBase(ctx, p.root, record, input, binding)
			if err != nil {
				return false, err
			}
			expected, err = reloadExpectation(input, base)
			if err != nil {
				return false, err
			}
			if observed.AckReadyCapture != "" {
				anchor, err := readAckReadiness(ctx, p.root, record, input, binding, p.session.ready, observed.AckReadyCapture, false)
				if err != nil {
					return false, err
				}
				expected.AfterSequence = anchor.Sequence - 1
			}
		}
		wait, cancel := context.WithTimeout(ctx, 15*time.Second)
		defer cancel()
		// Fresh pixels of the unchanged post-reload QR suffice; no input has
		// consumed that readiness yet. The native frame watermark never resets.
		signal, err := p.session.reader.WaitForSignal(wait, expected)
		if err != nil {
			if ctx.Err() == nil && (errors.Is(err, context.DeadlineExceeded) || errors.Is(err, io.EOF)) {
				return false, errors.Join(ErrAckReadinessPending, err)
			}
			return false, err
		}
		generic := signal.RequestID == "" && signal.ReloadNonce == ""
		correlated := signal.RequestID == input.Expected.RequestID && signal.ReloadNonce == input.Load.ReloadNonce
		if (!generic && !correlated) || signal.GUID != input.Load.GUID || signal.CodeBytes != 0 || signal.CodeAdler32 != "" || signal.ReportBytes != 0 || signal.ReportAdler32 != "" || signal.CleanupNonce != "" || signal.Sequence > 9007199254740991 {
			return false, errors.New("live.invalid_ack_readiness")
		}
		if err := p.check(ctx); err != nil {
			return false, err
		}
		raw, _ := json.Marshal(signal)
		capture, err := archive.CommitCapture(ctx, evidence.CaptureDraft{Reader: bytes.NewReader(raw), MaxBytes: 4096, MediaType: "application/json", Complete: true, Provenance: evidence.Provenance{Kind: "decoded-ack-readiness", Locator: input.Expected.RequestID, Snapshot: record.Intent.Snapshot, OperationID: p.id, DataBuild: input.Expected.Build, Session: input.Expected.SessionNonce}})
		if err != nil {
			return false, err
		}
		observed.AckReadyCapture = capture.ID
		raw, _ = json.Marshal(observed)
		if err := p.check(ctx); err != nil {
			return false, err
		}
		err = book.AdvanceStage(ctx, journal.StageChange{OperationID: p.id, ExpectedGeneration: record.Generation, ExpectedStage: "verified", Stage: "verified", Status: "running", Observation: raw})
		if err == nil {
			p.session.ready = signal
		}
		return true, err
	})
	return err
}
