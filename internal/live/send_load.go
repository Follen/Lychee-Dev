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

// Load requests compilation once after confirmed queue reentry. Observe must
// independently verify the resulting code receipt before Dispatch can execute.
func (p *ProbeOperation) Load(ctx context.Context) (desktop.InputReceipt, error) {
	return p.load(ctx, desktop.QueuePreparedCommand)
}

func (p *ProbeOperation) load(ctx context.Context, send preparedInput) (desktop.InputReceipt, error) {
	if err := p.check(ctx); err != nil {
		return desktop.InputReceipt{}, err
	}
	record, err := journal.OpenBook(p.metadata).InspectWork(ctx, p.id)
	if err != nil {
		return desktop.InputReceipt{}, err
	}
	var observed probeLoadObservation
	if err := json.Unmarshal(record.Observation, &observed); err != nil {
		return desktop.InputReceipt{}, err
	}
	if record.Stage != "load_requested" || record.Status != "running" || observed.LoadReadyCapture != "" || observed.BootstrapCapture == "" || observed.LoadedCapture != "" {
		return desktop.InputReceipt{}, journal.ErrTransition
	}
	return p.submitInput(ctx, "load_requested", send, p.prepareLoadInput)
}

func (p *ProbeOperation) prepareLoadInput(ctx context.Context) (string, error) {
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
		if record.Stage != "load_requested" || record.Status != "running" || observed.LoadReadyCapture != "" || observed.BootstrapCapture == "" || observed.LoadedCapture != "" {
			return "", journal.ErrTransition
		}
		input, binding, err := p.session.boundOperationInput(ctx, p.root, record)
		if err != nil {
			return "", err
		}
		ready, _, err := readBootstrapEvidence(ctx, p.root, record, input, binding, observed.bootstrapObservation)
		if err != nil {
			return "", err
		}
		_, definition, err := probeDefinition(record)
		if err != nil {
			return "", err
		}
		queue := delivery.AddonDirectory(p.session.target.Client.Directory)
		if _, err := delivery.VerifyProbePrepared(ctx, queue, definition); err != nil {
			return "", err
		}
		expected, err := bootstrapExpectation(input, binding)
		if err != nil {
			return "", err
		}
		expected.AfterSequence = ready.Sequence - 1
		wait, cancel := context.WithTimeout(ctx, 15*time.Second)
		defer cancel()
		signal, err := p.session.reader.WaitForSignal(wait, expected)
		if err != nil {
			return "", err
		}
		if err := validateBootstrap(signal, input, binding); err != nil {
			return "", err
		}
		deployment, _, err := delivery.ResolveAddonDestination(ctx, input.Load.Installation)
		if err != nil {
			return "", err
		}
		if deployment.Client.Product != input.Expected.Product || deployment.Client.FullBuild != input.Expected.Build {
			return "", errors.New("live.probe_installation_mismatch")
		}
		if _, err := delivery.VerifyProbePrepared(ctx, queue, definition); err != nil {
			return "", err
		}
		if err := p.check(ctx); err != nil {
			return "", err
		}
		raw, _ := json.Marshal(signal)
		provenance := bootstrapProvenance(record, input)
		provenance.Kind = "decoded-load-readiness"
		capture, err := evidence.OpenArchive(store, metadata).CommitCapture(ctx, evidence.CaptureDraft{Reader: bytes.NewReader(raw), MaxBytes: 4096, MediaType: "application/json", Complete: true, Provenance: provenance})
		if err != nil {
			return "", err
		}
		observed.LoadReadyCapture = capture.ID
		raw, _ = json.Marshal(observed)
		if err := book.AdvanceStage(ctx, journal.StageChange{OperationID: p.id, ExpectedGeneration: record.Generation, ExpectedStage: "load_requested", Stage: "load_requested", Status: "running", Observation: raw}); err != nil {
			return "", err
		}
		return "/dev bridge load " + input.Expected.RequestID, nil
	})
}
