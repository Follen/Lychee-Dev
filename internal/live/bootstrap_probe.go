package live

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"github.com/follenfang/lycheedev/internal/delivery"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/vault"
	"time"
)

// Bootstrap publishes the owned queue and submits one initial reload. It does
// not compile or execute the probe; ObserveBootstrap must confirm the handoff.
func (p *ProbeOperation) Bootstrap(ctx context.Context) (desktop.InputReceipt, error) {
	return p.bootstrap(ctx, desktop.QueuePreparedCommand)
}

func (p *ProbeOperation) bootstrap(ctx context.Context, send preparedInput) (desktop.InputReceipt, error) {
	if send == nil {
		return desktop.InputReceipt{}, errors.New("live.input_sender_missing")
	}
	if err := p.check(ctx); err != nil {
		return desktop.InputReceipt{}, err
	}
	record, err := journal.OpenBook(p.metadata).InspectWork(ctx, p.id)
	if err != nil {
		return desktop.InputReceipt{}, err
	}
	var observed probeLoadObservation
	if len(record.Observation) != 0 {
		if err := json.Unmarshal(record.Observation, &observed); err != nil {
			return desktop.InputReceipt{}, err
		}
	}
	if (record.Stage != "prepared" && record.Stage != "load_requested") || observed.BootstrapReadyID != "" || observed.LoadedCapture != "" || (record.Status != "pending" && record.Status != "running") {
		return desktop.InputReceipt{}, journal.ErrTransition
	}
	if _, err := p.PrepareFiles(ctx); err != nil {
		return desktop.InputReceipt{}, err
	}
	return p.submitInput(ctx, "load_requested", send, p.prepareBootstrapInput)
}

func (p *ProbeOperation) prepareBootstrapInput(ctx context.Context) (string, error) {
	return withOperation(ctx, p.root, p.id, func(store *vault.Store, metadata *vault.Metadata) (string, error) {
		if err := p.check(ctx); err != nil {
			return "", err
		}
		book := journal.OpenBook(metadata)
		record, err := book.InspectWork(ctx, p.id)
		if err != nil {
			return "", err
		}
		var observed probeLoadObservation
		if err := json.Unmarshal(record.Observation, &observed); err != nil {
			return "", err
		}
		if record.Stage != "load_requested" || record.Status != "running" || observed.BootstrapReadyID != "" || observed.LoadedCapture != "" || observed.Revision.SHA256 == "" {
			return "", journal.ErrTransition
		}
		input, binding, err := p.session.boundOperationInput(ctx, p.root, record)
		if err != nil {
			return "", err
		}
		if _, err := bootstrapExpectation(input, binding); err != nil {
			return "", err
		}
		deployment, _, err := delivery.ResolveAddonDestination(ctx, input.Load.Installation)
		if err != nil {
			return "", err
		}
		if deployment.Client.Product != input.Expected.Product || deployment.Client.FullBuild != input.Expected.Build {
			return "", errors.New("live.probe_installation_mismatch")
		}
		_, definition, err := probeDefinition(record)
		if err != nil {
			return "", err
		}
		queuePath := delivery.AddonDirectory(p.session.target.Client.Directory)
		if _, err := delivery.VerifyProbePrepared(ctx, queuePath, definition); err != nil {
			return "", err
		}
		expected := input.Expected
		expected.Kind, expected.RequestID, expected.ReloadNonce = "ready", "", ""
		expected.RuntimeEpoch, expected.AfterSequence, expected.RequireInputReady = binding.Ready.RuntimeEpoch, binding.Ready.Sequence-1, true
		wait, cancel := context.WithTimeout(ctx, 15*time.Second)
		defer cancel()
		signal, err := p.session.reader.WaitForSignal(wait, expected)
		if err != nil {
			return "", err
		}
		if signal.GUID != input.Load.GUID || signal.RequestID != "" || signal.ReloadNonce != "" || signal.CodeBytes != 0 || signal.CodeAdler32 != "" || signal.ReportBytes != 0 || signal.ReportAdler32 != "" {
			return "", errors.New("live.invalid_bootstrap_readiness")
		}
		if err := p.check(ctx); err != nil {
			return "", err
		}
		raw, _ := json.Marshal(signal)
		provenance := bootstrapProvenance(record, input)
		provenance.Kind = "decoded-bootstrap-readiness"
		capture, err := evidence.OpenArchive(store, metadata).CommitCapture(ctx, evidence.CaptureDraft{Reader: bytes.NewReader(raw), MaxBytes: 4096, MediaType: "application/json", Complete: true, Provenance: provenance})
		if err != nil {
			return "", err
		}
		observed.BootstrapReadyID = capture.ID
		raw, _ = json.Marshal(observed)
		if _, err := delivery.VerifyProbePrepared(ctx, queuePath, definition); err != nil {
			return "", err
		}
		if err := p.check(ctx); err != nil {
			return "", err
		}
		if err := book.AdvanceStage(ctx, journal.StageChange{OperationID: p.id, ExpectedGeneration: record.Generation, ExpectedStage: "load_requested", Stage: "load_requested", Status: "running", Observation: raw}); err != nil {
			return "", err
		}
		return "/dev bridge prepare " + input.Expected.RequestID + " " + input.Load.ReloadNonce, nil
	})
}

func (p *ProbeOperation) bootstrapContext(ctx context.Context) (journal.WorkRecord, ReportIntent, RecordedSession, probeLoadObservation, error) {
	var record journal.WorkRecord
	var input ReportIntent
	var binding RecordedSession
	var observed probeLoadObservation
	if p == nil || p.run == nil || p.metadata == nil {
		return record, input, binding, observed, errors.New("live.operation_closed")
	}
	record, err := p.run.Check(ctx)
	if err != nil {
		return record, input, binding, observed, err
	}
	if record.Stage != "load_requested" || (record.Status != "running" && record.Status != "unresolved") {
		return record, input, binding, observed, journal.ErrTransition
	}
	input, binding, err = p.session.boundOperationInput(ctx, p.root, record)
	if err != nil {
		return record, input, binding, observed, err
	}
	if err := json.Unmarshal(record.Observation, &observed); err != nil {
		return record, input, binding, observed, err
	}
	if observed.Revision.SHA256 == "" || observed.BootstrapReadyID == "" || observed.LoadedCapture != "" {
		return record, input, binding, observed, errors.New("live.bootstrap_intent_missing")
	}
	if _, err := bootstrapExpectation(input, binding); err != nil {
		return record, input, binding, observed, err
	}
	if p.session.ready.RuntimeEpoch != binding.Ready.RuntimeEpoch && p.session.ready.RuntimeEpoch != binding.Ready.RuntimeEpoch+1 {
		return record, input, binding, observed, errors.New("live.operation_runtime_mismatch")
	}
	if err := p.session.confirm(ctx, p.session.target); err != nil {
		return record, input, binding, observed, err
	}
	_, err = p.run.Check(ctx)
	return record, input, binding, observed, err
}

func (p *ProbeOperation) ObserveBootstrap(ctx context.Context) (evidence.CaptureRef, error) {
	if _, _, _, _, err := p.bootstrapContext(ctx); err != nil {
		return evidence.CaptureRef{}, err
	}
	return withOperation(ctx, p.root, p.id, func(store *vault.Store, metadata *vault.Metadata) (evidence.CaptureRef, error) {
		var zero evidence.CaptureRef
		record, input, binding, observed, err := p.bootstrapContext(ctx)
		if err != nil {
			return zero, err
		}
		_, definition, err := probeDefinition(record)
		if err != nil {
			return zero, err
		}
		queuePath := delivery.AddonDirectory(p.session.target.Client.Directory)
		if _, err := delivery.VerifyProbePrepared(ctx, queuePath, definition); err != nil {
			return zero, err
		}
		var existing evidence.CaptureRef
		if observed.BootstrapCapture != "" {
			_, existing, err = readBootstrapEvidence(ctx, p.root, record, input, binding, observed.bootstrapObservation)
			if err != nil {
				return zero, err
			}
		}
		expected, err := bootstrapExpectation(input, binding)
		if err != nil {
			return zero, err
		}
		wait, cancel := context.WithTimeout(ctx, 15*time.Second)
		defer cancel()
		signal, err := p.session.reader.WaitForSignal(wait, expected)
		if err != nil {
			return zero, err
		}
		if err := validateBootstrap(signal, input, binding); err != nil {
			return zero, err
		}
		if _, _, _, _, err := p.bootstrapContext(ctx); err != nil {
			return zero, err
		}
		if _, err := delivery.VerifyProbePrepared(ctx, queuePath, definition); err != nil {
			return zero, err
		}
		if existing.ID != "" {
			p.session.ready = signal
			return existing, nil
		}
		raw, _ := json.Marshal(signal)
		capture, err := evidence.OpenArchive(store, metadata).CommitCapture(ctx, evidence.CaptureDraft{Reader: bytes.NewReader(raw), MaxBytes: 4096, MediaType: "application/json", Complete: true, Provenance: bootstrapProvenance(record, input)})
		if err != nil {
			return zero, err
		}
		observed.BootstrapCapture = capture.ID
		// Verify the complete archived chain before committing the epoch handoff.
		if _, _, err := readBootstrapEvidence(ctx, p.root, record, input, binding, observed.bootstrapObservation); err != nil {
			return zero, err
		}
		raw, _ = json.Marshal(observed)
		if err := journal.OpenBook(metadata).AdvanceStage(ctx, journal.StageChange{OperationID: p.id, ExpectedGeneration: record.Generation, ExpectedStage: "load_requested", Stage: "load_requested", Status: "running", Observation: raw}); err != nil {
			return zero, err
		}
		p.session.ready = signal
		return capture, nil
	})
}
