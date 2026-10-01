package channel

import (
	"context"
	"encoding/json"
	"errors"
	"path/filepath"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/live/memory"
)

func attemptInputFixture(t *testing.T, next int, raw json.RawMessage) (memory.Record, bridge.SlotEnvelope) {
	t.Helper()
	blocked := false
	s := InputObservation{Schema: InputSchema, Runtime: strings.Repeat("1", 32), Owner: strings.Repeat("2", 32), Fence: 1, NextSlot: next, GUID: "g", Build: "b", SampleMillis: 1200, InputBlocked: &blocked}
	r := inputTestRecord(t, s)
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(r.Payload, &fields); err != nil {
		t.Fatal(err)
	}
	if raw != nil {
		fields["inputAttempt"] = raw
	}
	payload, err := json.Marshal(fields)
	if err != nil {
		t.Fatal(err)
	}
	wire, err := bridge.EncodeMemoryRecord(r.Header, payload)
	if err != nil {
		t.Fatal(err)
	}
	h, payload, err := bridge.DecodeMemoryRecord(wire)
	if err != nil {
		t.Fatal(err)
	}
	return memory.Record{Header: h, Payload: payload}, bridge.SlotEnvelope{Schema: bridge.SlotSchema, Index: 7, Nonce: strings.Repeat("3", 32), Runtime: s.Runtime, Owner: s.Owner, Fence: 1, GUID: s.GUID, Build: s.Build, Action: "commit"}
}

func attemptDiagnosticFixture() InputDiagnostic {
	return InputDiagnostic{Schema: InputDiagnosticSchema, Runtime: strings.Repeat("1", 32), AttemptSeq: 4, Slot: 7, Stage: "no_dispatch", Reason: "slot_loader_no_dispatch", Received: false}
}

func TestInputDiagnosticOptionalPreservesEligibilityAndErrors(t *testing.T) {
	d := attemptDiagnosticFixture()
	good, _ := json.Marshal(d)
	for _, raw := range []json.RawMessage{good, []byte(`"private raw suffix"`), []byte(`{"schema":"bad","received":true}`), []byte(`{"attemptSeq":"not a number"}`), []byte(`null`)} {
		r, e := attemptInputFixture(t, 7, raw)
		s, err := inputObservation(r, e, 1000, 1400)
		if err != nil || *s.InputBlocked {
			t.Fatalf("optional metadata changed eligibility: %v", err)
		}
		if (s.InputDiagnostic != nil) != (string(raw) == string(good)) {
			t.Fatal("invalid diagnostic retained")
		}
		e.Runtime = strings.Repeat("9", 32)
		if _, err = inputObservation(r, e, 1000, 1400); err == nil {
			t.Fatal("optional metadata bypassed original target")
		}
	}
	for _, edit := range []func(*InputDiagnostic){
		func(d *InputDiagnostic) { d.AttemptSeq = 0 }, func(d *InputDiagnostic) { d.AttemptSeq = 9007199254740992 },
		func(d *InputDiagnostic) { d.Slot = 202 }, func(d *InputDiagnostic) { d.Stage = "unknown" },
		func(d *InputDiagnostic) { d.Reason = "slot_load_failed: private raw suffix" },
		func(d *InputDiagnostic) { d.Stage = "received"; d.Reason = ""; d.Received = false },
		func(d *InputDiagnostic) { d.Runtime = strings.Repeat("0", 32) },
	} {
		d := attemptDiagnosticFixture()
		edit(&d)
		raw, _ := json.Marshal(d)
		if parseInputDiagnostic(raw, strings.Repeat("1", 32), 1200) != nil {
			t.Fatal("malformed diagnostic accepted")
		}
	}
	if parseInputDiagnostic([]byte(strings.Repeat(" ", 513)), d.Runtime, 1200) != nil {
		t.Fatal("oversized diagnostic accepted")
	}
}

func TestInputDiagnosticRequiresReceivedBoolean(t *testing.T) {
	for _, stage := range []string{"no_dispatch", "load_failed"} {
		for _, tc := range []struct {
			name  string
			value json.RawMessage
			valid bool
		}{
			{"missing", nil, false},
			{"null", json.RawMessage(`null`), false},
			{"number", json.RawMessage(`0`), false},
			{"string", json.RawMessage(`"false"`), false},
			{"boolean-false", json.RawMessage(`false`), true},
			{"boolean-true", json.RawMessage(`true`), true},
		} {
			t.Run(stage+"/"+tc.name, func(t *testing.T) {
				d := attemptDiagnosticFixture()
				d.Stage = stage
				if stage == "load_failed" {
					d.Reason = "slot_load_failed"
				}
				encoded, err := json.Marshal(d)
				if err != nil {
					t.Fatal(err)
				}
				var fields map[string]json.RawMessage
				if err = json.Unmarshal(encoded, &fields); err != nil {
					t.Fatal(err)
				}
				delete(fields, "received")
				if tc.value != nil {
					fields["received"] = tc.value
				}
				raw, err := json.Marshal(fields)
				if err != nil {
					t.Fatal(err)
				}
				if got := parseInputDiagnostic(raw, d.Runtime, 1200); (got != nil) != tc.valid {
					t.Fatalf("required bool valid=%t diagnostic=%+v", tc.valid, got)
				}
				// The optional diagnostic must not turn a bad field into a real
				// input error, nor suppress the original target mismatch.
				r, envelope := attemptInputFixture(t, 7, raw)
				s, err := inputObservation(r, envelope, 1000, 1400)
				if err != nil || *s.InputBlocked || (s.InputDiagnostic != nil) != tc.valid {
					t.Fatalf("optional bool changed input qualification: %v", err)
				}
				envelope.Runtime = strings.Repeat("9", 32)
				if _, err = inputObservation(r, envelope, 1000, 1400); err == nil {
					t.Fatal("optional bool hid the actual target error")
				}
			})
		}
	}
}

type attemptRecordReader struct {
	record memory.Record
	reads  int
	cancel context.CancelFunc
	err    error
}

func (r *attemptRecordReader) Find(_ context.Context, s memory.Selector, _ bool) (memory.LookupResult, error) {
	r.reads++
	if s.Kind != bridge.MemoryInputState {
		panic("diagnostic attempted another scan")
	}
	if r.cancel != nil {
		r.cancel()
	}
	return memory.LookupResult{Records: []memory.Record{r.record}}, r.err
}

func TestReceiptAttemptProbeOnceAndNeverAuthorizesAdvancedSlot(t *testing.T) {
	d := attemptDiagnosticFixture()
	d.Stage, d.Reason, d.Received = "received", "", true
	raw, _ := json.Marshal(d)
	r, e := attemptInputFixture(t, 8, raw)
	if _, err := inputObservation(r, e, 0, 1400); err == nil {
		t.Fatal("advanced INPUT authorized original slot")
	}
	reader := &attemptRecordReader{record: r}
	var probe receiptAttemptProbe
	q := ObservationQuery{Kind: "receipt", Envelope: e}
	for attempt := 0; attempt < 10; attempt++ {
		got, err := probe.observe(context.Background(), reader, q, Observation{}, ErrPending, 1400)
		if !errors.Is(err, ErrPending) || attempt == 0 && (got.InputDiagnostic == nil || !got.InputDiagnostic.Received) {
			t.Fatalf("attempt=%d err=%v", attempt, err)
		}
	}
	if reader.reads != 1 {
		t.Fatalf("repeated diagnostic reads=%d", reader.reads)
	}
	// A new native invocation may observe again without reusing an address.
	probe = receiptAttemptProbe{}
	if _, err := probe.observe(context.Background(), reader, q, Observation{}, ErrPending, 1800); !errors.Is(err, ErrPending) {
		t.Fatal(err)
	}
	if reader.reads != 2 {
		t.Fatal(reader.reads)
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	probe = receiptAttemptProbe{}
	if _, err := probe.observe(ctx, reader, q, Observation{}, ErrPending, 1400); !errors.Is(err, context.Canceled) || reader.reads != 2 {
		t.Fatal("cancel before read lost", err)
	}
	ctx, cancel = context.WithCancel(context.Background())
	reader.cancel = cancel
	if _, err := probe.observe(ctx, reader, q, Observation{}, ErrPending, 1400); !errors.Is(err, context.Canceled) {
		t.Fatal("cancel during read lost", err)
	}
}

type attemptPendingBackend struct {
	pendingBackend
	diagnostic *InputDiagnostic
}

func (b *attemptPendingBackend) Observe(context.Context, ObservationQuery) (Observation, error) {
	return Observation{InputDiagnostic: b.diagnostic}, ErrPending
}
func TestReceiptDiagnosticJournalOnceCannotConsumeOrRepeatInput(t *testing.T) {
	i := Identity{Schema: IdentitySchema, Runtime: strings.Repeat("1", 32), NextSlot: 7, Slots: bridge.SlotCount, GUID: "g", Character: "c", Realm: "r", Build: "b", Product: "retail", Release: "3.1.0"}
	backend := &attemptPendingBackend{}
	d, err := New(filepath.Join(t.TempDir(), "connection.jsonl"), backend, i)
	if err != nil {
		t.Fatal(err)
	}
	ctx := context.Background()
	if err = d.begin(ctx, "bind", strings.Repeat("0", 32), nil); err != nil {
		t.Fatal(err)
	}
	tx := d.State.Transaction
	tx.Phase = "input_attempted"
	before := attemptDiagnosticFixture()
	before.AttemptSeq = 3
	blocked := false
	d.State.Input = &InputAttempt{ID: strings.Repeat("5", 32), Exchange: tx.Envelope.Nonce, Runtime: i.Runtime, Kind: "invoke", Observation: &InputObservation{Runtime: i.Runtime, SampleMillis: 1000, InputBlocked: &blocked, InputDiagnostic: &before}, Outcome: &InputOutcome{Disposition: "submitted", MessagesQueued: 6, AtMillis: 1400}}
	after := attemptDiagnosticFixture()
	after.SampleMillis = 1200
	backend.diagnostic = &after
	if err = d.Save(ctx, "fixture"); err != nil {
		t.Fatal(err)
	}
	version := d.State.ProgressVersion
	for n := 0; n < 2; n++ {
		if _, err = d.receive(ctx, tx); !errors.Is(err, ErrPending) {
			t.Fatal(err)
		}
	}
	if d.State.ProgressVersion != version+1 || tx.Phase != "input_attempted" || tx.Receipt != nil || backend.sends != 0 || d.State.Input.AfterDiagnostic == nil {
		t.Fatal("diagnostic altered receipt/input authority")
	}
	loaded, err := Load(d.Log, backend)
	if err != nil {
		t.Fatal(err)
	}
	if loaded.State.Input.AfterDiagnostic.AttemptSeq != 4 || loaded.State.Transaction.Phase != "input_attempted" {
		t.Fatal("lost diagnostic or advanced unknown exchange")
	}
	loaded.State.Input.AfterDiagnostic.Reason = "private suffix"
	if loaded.State.validate() == nil {
		t.Fatal("invalid persisted diagnostic escaped its bound")
	}
}
