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

const IdentitySchema = "lycheedev.slot.identity.v2"
const InputSchema = "lycheedev.input.v3"

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
	if i.Schema != IdentitySchema {
		return errors.New("live.channel_identity_schema_invalid")
	}
	return i.validateFields()
}

// Receipts share identity fields but have their own wire schema.
func (r Receipt) Validate() error {
	if r.Schema != bridge.SlotSchema {
		return errors.New("live.channel_receipt_schema_invalid")
	}
	return r.Identity.validateFields()
}

func (i Identity) validateFields() error {
	if i.Inventory != nil && (*i.Inventory < 0 || *i.Inventory > i.Slots) {
		return errors.New("live.channel_inventory_invalid")
	}
	if _, err := tokenBytes(i.Runtime); err != nil {
		return err
	}
	if i.Slots != bridge.SlotCount || i.NextSlot < 1 || i.NextSlot > i.Slots+1 || i.GUID == "" || i.Character == "" || i.Realm == "" || i.Build == "" || i.Product == "" || i.Release == "" {
		return errors.New("live.channel_identity_invalid")
	}
	return nil
}

// A fresh identity legitimately has sequence zero. It is a typed discovery
// publication, never a nonce receipt or execution authorization.
func decodeIdentityRecord(record memory.Record, release string) (Identity, error) {
	var i Identity
	h := record.Header
	if h.Kind != bridge.MemoryIdentity || h.State != 1 || h.Nonce != [16]byte{} || h.Ticket != [16]byte{} || len(record.Payload) > 16384 || json.Unmarshal(record.Payload, &i) != nil || i.Validate() != nil || i.Release != release {
		return i, errors.New("live.channel_identity_record_invalid")
	}
	runtime, err := tokenBytes(i.Runtime)
	if err != nil || runtime == [16]byte{} || runtime != h.Runtime {
		return i, errors.New("live.channel_identity_record_invalid")
	}
	return i, nil
}

// Receipt and confirmation are typed HEAD publications. Their body tokens must
// agree with the checksummed header even when the ticket is the zero bind token.
func decodePublicationReceipt(record memory.Record) (Receipt, error) {
	var r Receipt
	h := record.Header
	if (h.Kind != bridge.MemoryReceipt && h.Kind != bridge.MemoryConfirmation) || h.State != 1 || h.Sequence == 0 || len(record.Payload) > 16384 {
		return r, errors.New("live.channel_receipt_record_invalid")
	}
	if err := json.Unmarshal(record.Payload, &r); err != nil {
		return r, err
	}
	if err := r.Validate(); err != nil {
		return r, err
	}
	nonce, nonceErr := tokenBytes(r.Nonce)
	runtime, runtimeErr := tokenBytes(r.Runtime)
	ticket, ticketErr := tokenBytes(r.Ticket)
	if nonceErr != nil || runtimeErr != nil || ticketErr != nil || nonce == [16]byte{} || runtime == [16]byte{} || h.Nonce != nonce || h.Runtime != runtime || h.Ticket != ticket || h.Kind == bridge.MemoryConfirmation && r.Action != "confirm" || h.Kind == bridge.MemoryReceipt && r.Action == "confirm" && r.State != "rejected" {
		return r, errors.New("live.channel_receipt_record_invalid")
	}
	return r, nil
}

func decodeReceipt(record memory.Record, e bridge.SlotEnvelope) (Receipt, error) {
	r, err := decodePublicationReceipt(record)
	if err != nil {
		return r, err
	}
	if r.Schema != e.Schema || r.Nonce != e.Nonce || r.Ticket != e.Ticket || r.Action != e.Action || r.Runtime != e.Runtime || r.GUID != e.GUID || r.Build != e.Build || r.NextSlot <= e.Index {
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
	ObserveInput(context.Context, bridge.SlotEnvelope, int64, string) (InputObservation, error)
	Input(context.Context, InputAction) (InputOutcome, error)
	RuntimeCandidate(context.Context, Identity) (*Identity, error)
	Supersede(context.Context, bridge.SlotEnvelope, Identity) error
	Consumed(context.Context, bridge.SlotEnvelope) error
}
