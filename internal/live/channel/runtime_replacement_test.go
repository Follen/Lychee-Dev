package channel

import (
	"strings"
	"testing"
)

func replacementFixture() (Identity, RuntimeReplacementProof) {
	old := Identity{Schema: IdentitySchema, Runtime: strings.Repeat("f", 32), Slots: 200, NextSlot: 1, GUID: "old", Character: "old", Realm: "r", Build: "b", Product: "retail", Release: "3", InputState: InputSchema}
	current := old
	current.Runtime = strings.Repeat("1", 32)
	current.GUID = "new"
	current.Character = "new"
	blocked := true
	o := InputObservation{Schema: InputSchema, Bindings: &InputBindings{}, Runtime: current.Runtime, GUID: "new", Build: "b", NextSlot: 1, InputBlocked: &blocked, Reason: "input_binding_unavailable", SampleMillis: 1200}
	first := o
	first.SampleMillis = 1100
	return old, RuntimeReplacementProof{Schema: RuntimeReplacementProofSchema, ProcessID: 1, ProcessStartedAt: 2, Current: current, Witness: RuntimeReplacementWitness{Address: 128, Length: 512, BeforeSHA256: strings.Repeat("1", 64), AfterSHA256: strings.Repeat("2", 64), First: first, FirstSequence: 1, Sequence: 2, Observation: o}}
}
func TestRuntimeReplacementProofRejectsUnsafeEvidence(t *testing.T) {
	old, p := replacementFixture()
	if err := p.Validate(old); err != nil {
		t.Fatal(err)
	}
	for name, mutate := range map[string]func(*RuntimeReplacementProof){
		"first_bindings_absent":  func(p *RuntimeReplacementProof) { p.Witness.First.Bindings = nil },
		"latest_bindings_absent": func(p *RuntimeReplacementProof) { p.Witness.Observation.Bindings = nil },
		"partial_bindings": func(p *RuntimeReplacementProof) {
			p.Witness.Observation.Bindings = &InputBindings{Wake: "ALT-CTRL-F11"}
		},
		"mixed_bindings": func(p *RuntimeReplacementProof) {
			p.Witness.Observation.Bindings = &InputBindings{Wake: "ALT-CTRL-F11", Submit: "ALT-CTRL-SHIFT-F12", Close: "ALT-CTRL-["}
		},
		"old_input_schema":      func(p *RuntimeReplacementProof) { p.Witness.Observation.Schema = "lycheedev.input.v2" },
		"old_hybrid_capability": func(p *RuntimeReplacementProof) { p.Current.InputState = "lycheedev.input.hybrid.v1" },
		"empty_unblocked": func(p *RuntimeReplacementProof) {
			ready := false
			p.Witness.Observation.InputBlocked = &ready
			p.Witness.Observation.Reason = ""
		},
		"first_schema":   func(p *RuntimeReplacementProof) { p.Witness.First.Schema = "lycheedev.input.v1" },
		"first_identity": func(p *RuntimeReplacementProof) { p.Witness.First.Runtime = old.Runtime },
		"same_sample":    func(p *RuntimeReplacementProof) { p.Witness.First.SampleMillis = p.Witness.Observation.SampleMillis },
		"same_sequence":  func(p *RuntimeReplacementProof) { p.Witness.FirstSequence = p.Witness.Sequence },
		"old_schema":     func(p *RuntimeReplacementProof) { p.Schema = "lycheedev.runtime-replacement.bytes.v1" }, "pid": func(p *RuntimeReplacementProof) { p.ProcessID = 0 }, "creation": func(p *RuntimeReplacementProof) { p.ProcessStartedAt = 0 }, "same_runtime": func(p *RuntimeReplacementProof) { p.Current.Runtime = old.Runtime }, "capability": func(p *RuntimeReplacementProof) { p.Current.InputState = "future" }, "sequence": func(p *RuntimeReplacementProof) { p.Witness.Sequence = 0 }, "sequence_wrap": func(p *RuntimeReplacementProof) { p.Witness.Sequence = ^uint32(0) }, "address": func(p *RuntimeReplacementProof) { p.Witness.Address = ^uint64(0) }, "payload_limit": func(p *RuntimeReplacementProof) { p.Witness.Length = 9999 }, "short": func(p *RuntimeReplacementProof) { p.Witness.Length = 1 }, "digest": func(p *RuntimeReplacementProof) { p.Witness.BeforeSHA256 = "x" }, "unchanged": func(p *RuntimeReplacementProof) { p.Witness.BeforeSHA256 = p.Witness.AfterSHA256 }, "owner": func(p *RuntimeReplacementProof) { p.Witness.Observation.Owner = strings.Repeat("2", 32) }, "fence": func(p *RuntimeReplacementProof) { p.Witness.Observation.Fence = 1 }, "runtime": func(p *RuntimeReplacementProof) { p.Witness.Observation.Runtime = old.Runtime }, "blocked": func(p *RuntimeReplacementProof) { p.Witness.Observation.InputBlocked = nil }, "negative_sample": func(p *RuntimeReplacementProof) { p.Witness.Observation.SampleMillis = -1 },
	} {
		t.Run(name, func(t *testing.T) {
			q := p
			mutate(&q)
			if q.Validate(old) == nil {
				t.Fatal("unsafe proof accepted")
			}
		})
	}
	// Runtime token lexical order is not authority; samples advance in one process clock.
	p.Witness.First.SampleMillis = 0
	p.Witness.Observation.SampleMillis = 1
	if p.Validate(old) != nil {
		t.Fatal("valid progressing sample rejected")
	}
}
