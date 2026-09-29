package channel

import (
	"encoding/hex"
	"encoding/json"
	"errors"
	"github.com/follenfang/lycheedev/internal/bridge"
	"strings"
)

const RuntimeReplacementProofSchema = "lycheedev.runtime-replacement.bytes.v1"

type RuntimeReplacementWitness struct {
	Address      uint64           `json:"address,string"`
	Length       uint32           `json:"length"`
	BeforeSHA256 string           `json:"beforeSHA256"`
	AfterSHA256  string           `json:"afterSHA256"`
	Sequence     uint32           `json:"sequence"`
	Observation  InputObservation `json:"observation"`
}
type RuntimeReplacementProof struct {
	Schema           string                    `json:"schema"`
	ProcessID        uint32                    `json:"processId"`
	ProcessStartedAt uint64                    `json:"processStartedAt"`
	Current          Identity                  `json:"current"`
	Witness          RuntimeReplacementWitness `json:"witness"`
}

// Validate checks the persisted shape of a trusted Native observation. Digests
// are audit evidence, not an independently verifiable external authorization.
// Authority assumes a single trusted producer, immutable nonmoving Lua strings,
// and no copying/revival of historical complete input records at new addresses.
func (p RuntimeReplacementProof) Validate(old Identity) error {
	bad := errors.New("live.channel_runtime_replacement_invalid")
	if p.Schema != RuntimeReplacementProofSchema || old.Validate() != nil || p.Current.Validate() != nil || p.ProcessID == 0 || p.ProcessStartedAt == 0 || p.Current.Runtime == old.Runtime || p.Current.Runtime == strings.Repeat("0", 32) || !observedInputCapability(p.Current.InputState) || p.Current.Build != old.Build || p.Current.Product != old.Product || p.Current.Release != old.Release {
		return bad
	}
	w := p.Witness
	o := w.Observation
	overhead := uint32(bridge.MemoryHeaderBytes + bridge.MemoryTrailerBytes)
	if w.Address == 0 || w.Length <= overhead || w.Length > overhead+2048 || w.Address > ^uint64(0)-uint64(w.Length) || w.Sequence == 0 || w.Sequence == ^uint32(0) {
		return bad
	}
	for _, digest := range []string{w.BeforeSHA256, w.AfterSHA256} {
		b, err := hex.DecodeString(digest)
		if err != nil || len(b) != 32 || digest != strings.ToLower(digest) {
			return bad
		}
	}
	if w.BeforeSHA256 == w.AfterSHA256 {
		return bad
	}
	if o.Schema != "lycheedev.input.v1" || o.Runtime != p.Current.Runtime || o.Owner != p.Current.Owner || o.Fence != p.Current.Fence || o.GUID != p.Current.GUID || o.Build != p.Current.Build || o.NextSlot != p.Current.NextSlot || !validReplacementObservation(o) {
		return bad
	}
	payload, err := json.Marshal(o)
	if err != nil || len(payload) > 2048 {
		return bad
	}
	return nil
}
func validReplacementObservation(o InputObservation) bool {
	if o.SampleMillis < 0 || o.SampleMillis > 1<<53-1 || o.NextSlot < 1 || o.NextSlot > 65 || o.GUID == "" || o.Build == "" || o.InputBlocked == nil || !*o.InputBlocked && o.Reason != "" || *o.InputBlocked && o.Reason == "" {
		return false
	}
	if o.Owner == "" {
		return o.Fence == 0
	}
	owner, err := tokenBytes(o.Owner)
	return err == nil && owner != [16]byte{} && o.Fence > 0 && o.Fence <= 1<<53-1
}
