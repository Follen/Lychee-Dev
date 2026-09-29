package channel

import (
	"strings"
	"testing"
)

func replacementFixture() (Identity, RuntimeReplacementProof) {
	old := Identity{Schema: "lycheedev.slot.identity.v1", Runtime: strings.Repeat("f", 32), Slots: 200, NextSlot: 1, GUID: "old", Character: "old", Realm: "r", Build: "b", Product: "retail", Release: "3", InputState: "lycheedev.input.v1"}
	current := old
	current.Runtime = strings.Repeat("1", 32)
	current.GUID = "new"
	current.Character = "new"
	blocked := true
	o := InputObservation{Schema: "lycheedev.input.v1", Runtime: current.Runtime, GUID: "new", Build: "b", NextSlot: 1, InputBlocked: &blocked, Reason: "input_binding_unavailable", SampleMillis: 1200}
	return old, RuntimeReplacementProof{Schema: RuntimeReplacementProofSchema, ProcessID: 1, ProcessStartedAt: 2, Current: current, Witness: RuntimeReplacementWitness{Address: 128, Length: 512, BeforeSHA256: strings.Repeat("1", 64), AfterSHA256: strings.Repeat("2", 64), Sequence: 1, Observation: o}}
}
func TestRuntimeReplacementProofRejectsUnsafeEvidence(t *testing.T) {
	old, p := replacementFixture()
	if err := p.Validate(old); err != nil {
		t.Fatal(err)
	}
	for name, mutate := range map[string]func(*RuntimeReplacementProof){
		"old_schema": func(p *RuntimeReplacementProof) { p.Schema = "" }, "pid": func(p *RuntimeReplacementProof) { p.ProcessID = 0 }, "creation": func(p *RuntimeReplacementProof) { p.ProcessStartedAt = 0 }, "same_runtime": func(p *RuntimeReplacementProof) { p.Current.Runtime = old.Runtime }, "capability": func(p *RuntimeReplacementProof) { p.Current.InputState = "future" }, "sequence": func(p *RuntimeReplacementProof) { p.Witness.Sequence = 0 }, "sequence_wrap": func(p *RuntimeReplacementProof) { p.Witness.Sequence = ^uint32(0) }, "address": func(p *RuntimeReplacementProof) { p.Witness.Address = ^uint64(0) }, "payload_limit": func(p *RuntimeReplacementProof) { p.Witness.Length = 9999 }, "short": func(p *RuntimeReplacementProof) { p.Witness.Length = 1 }, "digest": func(p *RuntimeReplacementProof) { p.Witness.BeforeSHA256 = "x" }, "unchanged": func(p *RuntimeReplacementProof) { p.Witness.BeforeSHA256 = p.Witness.AfterSHA256 }, "owner": func(p *RuntimeReplacementProof) { p.Witness.Observation.Owner = strings.Repeat("2", 32) }, "fence": func(p *RuntimeReplacementProof) { p.Witness.Observation.Fence = 1 }, "runtime": func(p *RuntimeReplacementProof) { p.Witness.Observation.Runtime = old.Runtime }, "blocked": func(p *RuntimeReplacementProof) { p.Witness.Observation.InputBlocked = nil }, "negative_sample": func(p *RuntimeReplacementProof) { p.Witness.Observation.SampleMillis = -1 },
	} {
		t.Run(name, func(t *testing.T) {
			q := p
			mutate(&q)
			if q.Validate(old) == nil {
				t.Fatal("unsafe proof accepted")
			}
		})
	}
	// Neither clock comparison nor token lexical order is authority.
	p.Witness.Observation.SampleMillis = 0
	if p.Validate(old) != nil {
		t.Fatal("sample clock used as authority")
	}
}
