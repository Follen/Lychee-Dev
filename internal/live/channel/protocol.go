// Package channel coordinates the memory/slot transport. The journal owns
// progress; native input and memory lookup supply evidence, never transitions.
package channel

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/live/memory"
)

var ErrPending = errors.New("live.channel_pending")
var ErrPublicationPending = errors.Join(ErrPending, errors.New("live.channel_shared_publication_pending"))

var ErrRuntimeChanged = errors.New("live.channel_runtime_changed")
var ErrReloadRequired = errors.New("live.channel_reload_required")
var ErrJournalMissing = errors.New("live.channel_journal_missing")
var ErrCleanupReloadRequired = errors.New("live.channel_cleanup_reload_required")

type RejectedError struct{ Reason string }

func (e *RejectedError) Error() string { return "live.channel_rejected: " + e.Reason }

type Identity struct {
	Schema     string `json:"schema"`
	Runtime    string `json:"runtime"`
	Owner      string `json:"owner"`
	Fence      uint64 `json:"fence"`
	NextSlot   int    `json:"nextSlot"`
	Slots      int    `json:"slots"`
	Inventory  *int   `json:"inventory,omitempty"`
	InputState string `json:"inputState,omitempty"`
	Character  string `json:"character"`
	Realm      string `json:"realm"`
	GUID       string `json:"guid"`
	Build      string `json:"build"`
	Product    string `json:"product"`
	Release    string `json:"release"`
}
type Receipt struct {
	Identity
	Nonce          string `json:"nonce"`
	Ticket         string `json:"ticket"`
	Action         string `json:"action"`
	State          string `json:"state"`
	Reason         string `json:"reason,omitempty"`
	PreparedNonce  string `json:"preparedNonce,omitempty"`
	Challenge      string `json:"challenge,omitempty"`
	ReportBytes    uint32 `json:"reportBytes,omitempty"`
	ReportChecksum uint32 `json:"reportChecksum,omitempty"`
}

func token() (string, error) {
	var b [16]byte
	if _, err := rand.Read(b[:]); err != nil {
		return "", err
	}
	return hex.EncodeToString(b[:]), nil
}
func tokenBytes(s string) ([16]byte, error) {
	var b [16]byte
	raw, err := hex.DecodeString(s)
	if err != nil || len(raw) != 16 || hex.EncodeToString(raw) != s {
		return b, errors.New("live.channel_token_invalid")
	}
	copy(b[:], raw)
	return b, nil
}
func (i Identity) Validate() error {
	if i.Inventory != nil && (*i.Inventory < 0 || *i.Inventory > 64) {
		return errors.New("live.channel_inventory_invalid")
	}
	if _, err := tokenBytes(i.Runtime); err != nil {
		return err
	}
	if i.Slots != 64 || i.NextSlot < 1 || i.NextSlot > 65 || i.GUID == "" || i.Character == "" || i.Realm == "" || i.Build == "" || i.Product == "" || i.Release == "" {
		return errors.New("live.channel_identity_invalid")
	}
	return nil
}
func decodeReceipt(record memory.Record, e bridge.SlotEnvelope) (Receipt, error) {
	var r Receipt
	if err := json.Unmarshal(record.Payload, &r); err != nil {
		return r, err
	}
	if err := r.Validate(); err != nil {
		return r, err
	}
	if r.Schema != bridge.SlotSchema || r.Nonce != e.Nonce || r.Ticket != e.Ticket || r.Action != e.Action || r.Runtime != e.Runtime || r.GUID != e.GUID || r.Build != e.Build || r.NextSlot <= e.Index {
		return r, errors.New("live.channel_receipt_mismatch")
	}
	if r.Owner != e.Owner || r.Fence != e.Fence {
		return r, errors.New("live.channel_owner_mismatch")
	}
	if r.State == "rejected" {
		return r, &RejectedError{Reason: r.Reason}
	}
	return r, nil
}

// Backend's publication reservation survives process death. Send is at most one
// attempted chord; retransmission requires a separate current-slot proof.
type Backend interface {
	Observe(context.Context, ObservationQuery) (Observation, error)
	Publish(context.Context, bridge.SlotEnvelope) error
	ObserveInput(context.Context, bridge.SlotEnvelope, int64) (InputObservation, error)
	Input(context.Context, InputAction) (InputOutcome, error)
	RuntimeCandidate(context.Context, Identity) (*Identity, error)
	Supersede(context.Context, bridge.SlotEnvelope, Identity) error
	Consumed(context.Context, bridge.SlotEnvelope) error
}
