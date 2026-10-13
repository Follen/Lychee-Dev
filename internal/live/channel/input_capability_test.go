package channel

import (
	"context"
	"errors"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/live/memory"
)

func TestInputCapabilityRouting(t *testing.T) {
	if !observedInputCapability(InputSchema) || inputCapabilityError(InputSchema) != nil {
		t.Fatal("mailbox input rejected")
	}
	for _, capability := range []string{"", "future", "lycheedev.input.v1", "lycheedev.input.v2", "lycheedev.input.v3", "lycheedev.input.hybrid.v1", "lycheedev.input.hybrid.v2"} {
		if observedInputCapability(capability) || inputCapabilityError(capability) == nil {
			t.Fatalf("retired or unknown input capability accepted: %s", capability)
		}
	}
	if inputActionCapabilityError("", "reload_fallback") == nil || inputActionCapabilityError("", "reload") == nil {
		t.Fatal("first-install activation contract changed")
	}
	for _, capability := range []string{legacyInputSchema, "lycheedev.input.hybrid.v2"} {
		for _, action := range []string{"bind", "prepare", "commit", "confirm", "release", "unbind", "reload", "reload_fallback"} {
			if inputActionCapabilityError(capability, action) == nil {
				t.Fatalf("legacy capability %s authorized %s incorrectly", capability, action)
			}
		}
	}
}

func TestProtocolRenameAuthorizesOnlyObservedTwoKeyReload(t *testing.T) {
	ready := false
	e := bridge.SlotEnvelope{Runtime: strings.Repeat("1", 32), GUID: "g", Build: "b", Index: 1}
	s := InputObservation{Schema: bridge.PreviousMailboxSchema, Runtime: e.Runtime, GUID: e.GUID, Build: e.Build, NextSlot: 1, InputBlocked: &ready, Bindings: primaryInputBindings(), SampleMillis: 1200}
	for _, action := range []string{"reload", "invoke", "bind", "prepare", "commit", "confirm", "release", "unbind", "escape", "reload_fallback"} {
		e.Action = action
		_, err := inputObservation(inputTestRecord(t, s), e, 1000, 1250)
		if (err == nil) != (action == "reload") || (inputActionCapabilityError(s.Schema, action) == nil) != (action == "reload") {
			t.Fatal(action, err)
		}
	}
	e.Action = "reload"
	s.EscapeLimit = 3
	if _, err := inputObservation(inputTestRecord(t, s), e, 1000, 1250); err == nil {
		t.Fatal("old Esc scheme authorized reload")
	}
	s.EscapeLimit = 0
	s.Bindings = &InputBindings{Wake: "ALT-CTRL-F12", Submit: "ALT-CTRL-SHIFT-F12", Close: "ALT-CTRL-["}
	if _, err := inputObservation(inputTestRecord(t, s), e, 1000, 1250); err == nil {
		t.Fatal("three-key receiver authorized reload")
	}
}

func TestActivationDistinguishesPreviousThreeKeyAndTwoKeyRuntimes(t *testing.T) {
	i := Identity{Runtime: strings.Repeat("1", 32), GUID: "g", Build: "b", NextSlot: 1, InputState: bridge.PreviousMailboxSchema}
	ready := false
	base := InputObservation{Schema: bridge.PreviousMailboxSchema, Runtime: i.Runtime, GUID: i.GUID, Build: i.Build, NextSlot: 1, InputBlocked: &ready, Bindings: primaryInputBindings(), SampleMillis: 1200}
	for _, tc := range []struct {
		name string
		edit func(*InputObservation)
		want bool
	}{
		{"two-key", func(*InputObservation) {}, true},
		{"three-key-era", func(s *InputObservation) {
			s.Bindings = &InputBindings{Wake: "ALT-CTRL-F12", Submit: "ALT-CTRL-SHIFT-F12", Close: "ALT-CTRL-["}
		}, false},
		{"stale-two-key-capability", func(s *InputObservation) { s.SampleMillis = 100 }, true},
		{"foreign-character", func(s *InputObservation) { s.GUID = "other" }, false},
		{"unavailable-bindings", func(s *InputObservation) {
			blocked := true
			s.InputBlocked, s.Reason, s.Bindings = &blocked, "input_binding_unavailable", &InputBindings{}
		}, false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			s := base
			tc.edit(&s)
			reader := &activationInputReader{record: inputTestRecord(t, s)}
			got, err := activationInputCapability(context.Background(), reader, i, 1250)
			if err != nil || got != tc.want || reader.calls != 1 {
				t.Fatalf("capability=%v want=%v reads=%d err=%v", got, tc.want, reader.calls, err)
			}
			if tc.name == "stale-two-key-capability" {
				e := bridge.SlotEnvelope{Runtime: i.Runtime, GUID: i.GUID, Build: i.Build, Action: "reload"}
				if _, err := inputObservation(reader.record, e, 0, 1250); !errors.Is(err, errInputStateStale) {
					t.Fatal("capability check bypassed input freshness", err)
				}
			}
		})
	}
}

type activationInputReader struct {
	record memory.Record
	calls  int
}

func (r *activationInputReader) Find(_ context.Context, s memory.Selector, _ bool) (memory.LookupResult, error) {
	r.calls++
	if s.Kind != bridge.MemoryInputState || s.Runtime != r.record.Header.Runtime || s.Nonce != s.Runtime {
		return memory.LookupResult{}, errors.New("incorrect activation INPUT selector")
	}
	return memory.LookupResult{Records: []memory.Record{r.record}}, nil
}

func TestMailboxInputLossDoesNotSuppressRuntimeDiscovery(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	peer := &mailboxInputRecoveryPeer{stop: cancel}
	i := Identity{Schema: IdentitySchema, Runtime: strings.Repeat("1", 32), NextSlot: 1, Slots: 500, GUID: "g", Character: "c", Realm: "r", Build: "b", Product: "retail", Release: "3.0.0", InputState: InputSchema}
	d, err := New(filepath.Join(t.TempDir(), "input.jsonl"), peer, i)
	if err != nil {
		t.Fatal(err)
	}
	err = d.Continue(ctx)
	if !errors.Is(err, ErrPending) || peer.discoveries != 1 || peer.sends != 0 {
		t.Fatal("missing input prevented recovery or caused input", err, peer.discoveries, peer.sends)
	}
}

type mailboxInputRecoveryPeer struct {
	pendingBackend
	stop        context.CancelFunc
	discoveries int
}

func (p *mailboxInputRecoveryPeer) ObserveInput(context.Context, bridge.SlotEnvelope, int64, string) (InputObservation, error) {
	return InputObservation{}, ErrPending
}
func (p *mailboxInputRecoveryPeer) RuntimeCandidate(context.Context, Identity) (*Identity, error) {
	p.discoveries++
	p.stop()
	return nil, nil
}
