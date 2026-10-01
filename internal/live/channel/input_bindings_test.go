package channel

import (
	"encoding/json"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/bridge"
)

func TestInputBindingsRequiredExactProfiles(t *testing.T) {
	blocked := false
	e := bridge.SlotEnvelope{Runtime: strings.Repeat("1", 32), Owner: "owner", Fence: 1, Index: 1, GUID: "g", Build: "b"}
	base := InputObservation{Schema: InputSchema, Runtime: e.Runtime, Owner: e.Owner, Fence: e.Fence, NextSlot: e.Index, GUID: e.GUID, Build: e.Build, SampleMillis: 1200, InputBlocked: &blocked, Bindings: primaryInputBindings()}
	for _, tc := range []struct {
		name, raw string
		valid     bool
	}{
		{"primary", `{"wake":"ALT-CTRL-F12","submit":"ALT-CTRL-SHIFT-F12","close":"ALT-CTRL-["}`, true},
		{"fallback", `{"wake":"ALT-CTRL-F11","submit":"ALT-CTRL-SHIFT-F11","close":"ALT-CTRL-["}`, true},
		{"absent", "", false}, {"null", "null", false}, {"empty-object", "{}", false},
		{"partial", `{"wake":"ALT-CTRL-F11","close":"ALT-CTRL-["}`, false},
		{"mixed", `{"wake":"ALT-CTRL-F11","submit":"ALT-CTRL-SHIFT-F12","close":"ALT-CTRL-["}`, false},
		{"arbitrary", `{"wake":"ALT-CTRL-F1","submit":"ALT-CTRL-SHIFT-F1","close":"ALT-CTRL-["}`, false},
		{"empty-ready", `{"wake":"","submit":"","close":""}`, false},
		{"null-member", `{"wake":null,"submit":"","close":""}`, false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			record := inputTestRecord(t, base)
			var payload map[string]json.RawMessage
			if err := json.Unmarshal(record.Payload, &payload); err != nil {
				t.Fatal(err)
			}
			if tc.raw == "" {
				delete(payload, "bindings")
			} else {
				payload["bindings"] = json.RawMessage(tc.raw)
			}
			record.Payload, _ = json.Marshal(payload)
			s, err := inputObservation(record, e, 1000, 1250)
			if (err == nil) != tc.valid {
				t.Fatalf("valid=%v error=%v", tc.valid, err)
			}
			if tc.valid {
				profile, err := s.receiverProfile()
				if err != nil || profile.WakeBinding != s.Bindings.Wake || profile.SubmitBinding != s.Bindings.Submit {
					t.Fatalf("actual profile not selected: %#v %v", profile, err)
				}
			}
		})
	}
	base.Schema = "lycheedev.input.v2"
	if _, err := inputObservation(inputTestRecord(t, base), e, 1000, 1250); err == nil {
		t.Fatal("old INPUT schema accepted")
	}
	for _, capability := range []string{"lycheedev.input.v2", "lycheedev.input.hybrid.v1"} {
		if observedInputCapability(capability) || inputCapabilityError(capability) == nil {
			t.Fatalf("old capability accepted: %s", capability)
		}
	}
}

func TestInputBindingsUnavailableDoesNotAuthorizeKeys(t *testing.T) {
	blocked := true
	s := InputObservation{Schema: InputSchema, NextSlot: 1, GUID: "g", Build: "b", InputBlocked: &blocked, Reason: "input_binding_unavailable", Bindings: &InputBindings{}}
	if !validReplacementObservation(s) {
		t.Fatal("blocked runtime cannot prove retirement")
	}
	if _, err := s.receiverProfile(); err == nil {
		t.Fatal("unavailable profile authorized keys")
	}
	s.Bindings = nil
	if validReplacementObservation(s) {
		t.Fatal("missing profile accepted for retirement")
	}
	s.Bindings = primaryInputBindings()
	if validReplacementObservation(s) {
		t.Fatal("unavailable reason accepted with active profile")
	}
	for _, reason := range []string{"input_keyboard_focus", "input_combat_lockdown"} {
		s.Reason = reason
		if !s.validBindings() {
			t.Fatalf("blocked %s lost valid profile", reason)
		}
		s.Bindings = &InputBindings{}
		if s.validBindings() {
			t.Fatalf("blocked %s accepted empty profile", reason)
		}
		s.Bindings = primaryInputBindings()
	}
}

func TestInputBindingsChangedAtSameSampleRejectsAuthorization(t *testing.T) {
	blocked := false
	stored := InputObservation{Schema: InputSchema, InputBlocked: &blocked, SampleMillis: 1200, Bindings: primaryInputBindings()}
	current := stored
	if !sameInputProfile(current, stored) {
		t.Fatal("unchanged profile rejected")
	}
	current.Bindings = fallbackInputBindings()
	if sameInputProfile(current, stored) {
		t.Fatal("changed profile with identical sample accepted")
	}
	stored.Bindings = fallbackInputBindings()
	if !sameInputProfile(current, stored) {
		t.Fatal("verified fallback rejected")
	}
	profile, err := stored.receiverProfile()
	if err != nil || profile.WakeBinding != "ALT-CTRL-F11" || profile.SubmitBinding != "ALT-CTRL-SHIFT-F11" || profile.CloseBinding != "ALT-CTRL-[" {
		t.Fatalf("fallback profile = %#v, %v", profile, err)
	}
}
