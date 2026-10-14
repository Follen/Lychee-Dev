// Historical records only: no game capture, input, or execution adapters.
package live

import (
	"context"
	"encoding/hex"
	"encoding/json"
	"errors"
	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"path/filepath"
	"time"
)

type ReportIntent struct {
	Schema         string                   `json:"schema"`
	Revision       string                   `json:"revision,omitempty"`
	BudgetSeconds  int                      `json:"budgetSeconds,omitempty"`
	DeclaredBudget bool                     `json:"-"`
	Expected       bridge.SignalExpectation `json:"expected"`
	Code           []byte                   `json:"code"`
	Load           *ProbeLoadIntent         `json:"load,omitempty"`
	Binding        string                   `json:"binding,omitempty"`
}

type ProbeLoadIntent struct {
	Installation string `json:"installation"`
	Account      string `json:"account,omitempty"`
	ReportScope  string `json:"reportScope,omitempty"`
	GUID         string `json:"guid"`
	ReloadNonce  string `json:"reloadNonce"`
}

type bootstrapObservation struct {
	// Committed before sending input. Presence means reconcile, never resend;
	// BootstrapInput is diagnostic and may be absent after process interruption.
	BootstrapReadyID string                `json:"bootstrapReadyId,omitempty"`
	BootstrapCapture string                `json:"bootstrapCapture,omitempty"`
	BootstrapInput   *desktop.InputReceipt `json:"bootstrapInput,omitempty"`
}

func reportScope(input ReportIntent) string {
	if input.Load == nil {
		return ""
	}
	return input.Load.ReportScope
}

func reportInput(record journal.WorkRecord) (ReportIntent, error) {
	var input ReportIntent
	if err := json.Unmarshal(record.Intent.Request, &input); err != nil {
		return input, err
	}
	if input.Schema != "lycheedev.report-intent.v1" || record.Intent.Snapshot == "" || record.Intent.Session == "" || input.Expected.SessionNonce != record.Intent.Session {
		return input, errors.New("live.invalid_report_intent")
	}
	input.DeclaredBudget = input.BudgetSeconds != 0
	if reportScope(input) != "" && reportScope(input) != "character-v1" {
		return input, errors.New("bridge.invalid_report_scope")
	}
	if input.BudgetSeconds == 0 { // Retained 2.0 records predate declared budgets.
		input.BudgetSeconds = 120
	}
	if input.BudgetSeconds < 1 || input.BudgetSeconds > 120 {
		return input, errors.New("live.execution_budget_invalid")
	}
	var bootstrap bootstrapObservation
	if len(record.Observation) != 0 {
		if err := json.Unmarshal(record.Observation, &bootstrap); err != nil {
			return input, err
		}
	}
	// Sequence floors are local to a Lua runtime. Phase receipts still impose
	// their own loaded/reported floors; the initial pre-reload floor cannot.
	if bootstrap.BootstrapCapture != "" {
		input.Expected.AfterSequence = 0
	}
	return input, nil
}

type reportObservation struct {
	bootstrapObservation
	Schema            string          `json:"schema"`
	SourcePath        string          `json:"sourcePath,omitempty"`
	SourceSHA256      string          `json:"sourceSHA256,omitempty"`
	BodyID            string          `json:"bodyId"`
	ReceiptID         string          `json:"receiptId"`
	AcknowledgementID string          `json:"acknowledgementId,omitempty"`
	RemovalID         string          `json:"removalId,omitempty"`
	RemovalSHA256     string          `json:"removalSHA256,omitempty"`
	QueueRetirement   json.RawMessage `json:"queueRetirement,omitempty"`
	CleanupNonce      string          `json:"cleanupNonce,omitempty"`
	ClearedID         string          `json:"clearedId,omitempty"`
	ReloadedCapture   string          `json:"reloadedCapture,omitempty"`
	// Submission is diagnostic, not execution evidence. Preserve the earlier
	// transport chain through verification and ACK; old records may omit it.
	LoadedCapture     string                `json:"loadedCapture,omitempty"`
	LoadReadyCapture  string                `json:"loadReadyCapture,omitempty"`
	ReportedCapture   string                `json:"reportedCapture,omitempty"`
	FlushReadyCapture string                `json:"flushReadyCapture,omitempty"`
	LoadInput         *desktop.InputReceipt `json:"loadInput,omitempty"`
	DispatchInput     *desktop.InputReceipt `json:"dispatchInput,omitempty"`
	FlushInput        *desktop.InputReceipt `json:"flushInput,omitempty"`
	AckReadyCapture   string                `json:"ackReadyCapture,omitempty"`
	AckInput          *desktop.InputReceipt `json:"ackInput,omitempty"`
	CleanupReadyID    string                `json:"cleanupReadyId,omitempty"`
	CleanupInput      *desktop.InputReceipt `json:"cleanupInput,omitempty"`
}

type probeLoadObservation struct {
	bootstrapObservation
	Schema               string                `json:"schema"`
	Revision             json.RawMessage       `json:"revision"`
	LoadedCapture        string                `json:"loadedCapture,omitempty"`
	LoadReadyCapture     string                `json:"loadReadyCapture,omitempty"`
	LoadInput            *desktop.InputReceipt `json:"loadInput,omitempty"`
	ReportedCapture      string                `json:"reportedCapture,omitempty"`
	ReportErrorCapture   string                `json:"reportErrorCapture,omitempty"`
	DispatchInput        *desktop.InputReceipt `json:"dispatchInput,omitempty"`
	ExecutionRequestedAt time.Time             `json:"executionRequestedAt,omitempty"`
	FlushInput           *desktop.InputReceipt `json:"flushInput,omitempty"`
	FlushReadyCapture    string                `json:"flushReadyCapture,omitempty"`
	ReloadedCapture      string                `json:"reloadedCapture,omitempty"`
}

type fixedReloadIntent struct {
	Schema         string                   `json:"schema"`
	Target         ClientWindow             `json:"target"`
	Release        string                   `json:"release"`
	Commit         string                   `json:"commit"`
	IdentityNonce  string                   `json:"identityNonce"`
	OldEpoch       uint64                   `json:"oldEpoch,omitempty"`
	OldReportScope string                   `json:"oldReportScope,omitempty"`
	OldGUID        string                   `json:"oldGuid,omitempty"`
	OldCharacter   string                   `json:"oldCharacter,omitempty"`
	OldRealm       string                   `json:"oldRealm,omitempty"`
	Bindings       desktop.ReceiverBindings `json:"bindings,omitempty"`
	KnownBindings  bool                     `json:"knownBindings,omitempty"`
}

type fixedReloadObservation struct {
	Schema           string                `json:"schema"`
	DismissRequested bool                  `json:"dismissRequested,omitempty"`
	CloseQueued      bool                  `json:"closeQueued,omitempty"`
	ReleaseAt        time.Time             `json:"releaseAt,omitempty"`
	ReceiverReleased bool                  `json:"receiverReleased,omitempty"`
	NextStep         int                   `json:"nextStep,omitempty"`
	InputComplete    bool                  `json:"inputComplete,omitempty"`
	Input            *desktop.InputReceipt `json:"input,omitempty"`
	IdentityCapture  string                `json:"identityCapture,omitempty"`
}

func parseFixedReload(record journal.WorkRecord) (fixedReloadIntent, fixedReloadObservation, error) {
	var intent fixedReloadIntent
	var observed fixedReloadObservation
	if record.Intent.Kind != "reload" || json.Unmarshal(record.Intent.Request, &intent) != nil || intent.Schema != "lycheedev.fixed-reload.v1" || intent.Release == "" || intent.IdentityNonce == "" || intent.Target.Client.Directory == "" || intent.Target.Window.ProcessID == 0 || intent.Target.Window.ProcessStartedAt == 0 {
		return intent, observed, errors.New("live.fixed_reload_intent_invalid")
	}
	if len(record.Observation) != 0 && string(record.Observation) != "null" {
		if json.Unmarshal(record.Observation, &observed) != nil || observed.Schema != "lycheedev.fixed-reload-observation.v1" || observed.NextStep < 0 || observed.NextStep > 6 {
			return intent, observed, errors.New("live.fixed_reload_observation_invalid")
		}
	}
	return intent, observed, nil
}

func fixedReloadOutcome(ctx context.Context, archive *evidence.Archive, record journal.WorkRecord, result Outcome) (Outcome, error) {
	intent, observed, err := parseFixedReload(record)
	if err != nil {
		return result, err
	}
	if record.Stage != "cleaned" || record.Status != "completed" {
		return result, nil
	}
	ref, raw, err := archive.FetchCapture(ctx, observed.IdentityCapture, 4096)
	if err != nil {
		return result, err
	}
	if ref.Provenance.Kind != "decoded-fixed-reload" || ref.Provenance.Locator != intent.IdentityNonce || ref.Provenance.OperationID != record.OperationID || !ref.Complete || ref.Truncated {
		return result, errors.New("live.fixed_reload_evidence_mismatch")
	}
	signal, err := bridge.ParseSignal(raw)
	if err != nil {
		return result, err
	}
	if signal.Kind != "identity" || signal.ProbeNonce != intent.IdentityNonce || signal.Release != intent.Release || signal.Product != intent.Target.Client.Product || signal.Build != intent.Target.Client.FullBuild {
		return result, errors.New("live.fixed_reload_evidence_mismatch")
	}
	result.Complete = true
	result.RuntimeCapture = ref.ID
	return result, nil
}

type standaloneReloadIntent struct {
	Schema        string                   `json:"schema"`
	Binding       string                   `json:"binding"`
	Installation  string                   `json:"installation"`
	GUID          string                   `json:"guid"`
	ReloadNonce   string                   `json:"reloadNonce"`
	Expected      bridge.SignalExpectation `json:"expected"`
	FromRelease   string                   `json:"fromRelease,omitempty"`
	ManagedCommit string                   `json:"managedCommit,omitempty"`
}

type standaloneReloadObservation struct {
	Schema       string                `json:"schema"`
	Input        *desktop.InputReceipt `json:"input,omitempty"`
	ReadyCapture string                `json:"readyCapture,omitempty"`
}

func parseStandaloneReload(record journal.WorkRecord) (standaloneReloadIntent, error) {
	var input standaloneReloadIntent
	if record.Intent.Kind != "reload" || json.Unmarshal(record.Intent.Request, &input) != nil || input.Schema != "lycheedev.reload-intent.v1" || input.Binding == "" || !filepath.IsAbs(input.Installation) || len(input.ReloadNonce) != 32 || input.Expected.RequestID != "RELOAD-"+input.ReloadNonce || input.Expected.ReloadNonce != input.ReloadNonce || input.Expected.Kind != "ready" {
		return input, errors.New("live.invalid_reload_intent")
	}
	if _, err := hex.DecodeString(input.ReloadNonce); err != nil {
		return input, errors.New("live.invalid_reload_intent")
	}
	return input, nil
}

func reloadOutcome(ctx context.Context, archive *evidence.Archive, record journal.WorkRecord, result Outcome) (Outcome, error) {
	input, err := parseStandaloneReload(record)
	if err != nil {
		return result, err
	}
	if record.Stage != "cleaned" || record.Status != "completed" {
		return result, nil
	}
	var observed standaloneReloadObservation
	if json.Unmarshal(record.Observation, &observed) != nil || observed.Schema != "lycheedev.reload-observation.v1" || observed.ReadyCapture == "" {
		return result, errors.New("live.invalid_reload_receipt")
	}
	ref, data, err := archive.FetchCapture(ctx, observed.ReadyCapture, 4096)
	if err != nil {
		return result, err
	}
	want := evidence.Provenance{Kind: "decoded-standalone-reload", Locator: input.ReloadNonce, Snapshot: record.Intent.Snapshot, OperationID: record.OperationID, DataBuild: input.Expected.Build, Session: input.Expected.SessionNonce}
	if ref.Provenance != want || !ref.Complete || ref.Truncated || ref.MediaType != "application/json" {
		return result, errors.New("live.invalid_reload_receipt")
	}
	signal, err := bridge.ParseSignal(data)
	if err != nil {
		return result, err
	}
	if err := signal.Match(input.Expected); err != nil {
		return result, err
	}
	if signal.GUID != input.GUID || signal.CodeBytes != 0 || signal.ReportBytes != 0 || signal.CleanupNonce != "" {
		return result, errors.New("live.invalid_reload_receipt")
	}
	result.Complete = true
	result.RuntimeCapture = ref.ID
	return result, nil
}
