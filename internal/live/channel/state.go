package channel

import (
	"encoding/hex"
	"encoding/json"
	"errors"
	"hash/adler32"
	"strings"

	"github.com/follenfang/lycheedev/internal/bridge"
)

func (op *Operation) UnmarshalJSON(data []byte) error {
	type plain Operation
	var value struct {
		plain
		Legacy json.RawMessage `json:"result"`
	}
	if err := json.Unmarshal(data, &value); err != nil {
		return err
	}
	*op = Operation(value.plain)
	if op.Result == nil && len(value.Legacy) > 0 {
		op.Result = append([]byte(nil), value.Legacy...)
	}
	return nil
}

func (s State) validate() error {
	bad := errors.New("live.channel_journal_invalid")
	if s.ProcessEnd != "" && s.ProcessEnd != "process_absent" && s.ProcessEnd != "process_exited" && s.ProcessEnd != "pid_reused" {
		return bad
	}
	if in := s.Input; in != nil {
		if _, err := tokenBytes(in.ID); err != nil {
			return bad
		}
		if _, err := tokenBytes(in.Runtime); err != nil {
			return bad
		}
		if in.Kind != "invoke" && in.Kind != "escape" && in.Kind != "reload" {
			return bad
		}
		if in.Exchange == "" || in.Outcome != nil && in.Outcome.validate() != nil {
			return bad
		}
	}
	if s.Archive != "" {
		if b, err := hex.DecodeString(s.Archive); err != nil || len(b) != 32 {
			return bad
		}
	}
	if s.Schema != "lycheedev.channel.v1" || s.ID != "CON-"+s.Owner || s.Identity.Validate() != nil {
		return bad
	}
	if s.Closed && (s.Bound || s.Transaction != nil || s.Operation != nil && s.Operation.Stage != "complete" && s.Operation.Stage != "execution_unknown") {
		return bad
	}
	if _, err := tokenBytes(s.Owner); err != nil {
		return bad
	}
	if r := s.Recovery; r != nil {
		if r.From.Validate() != nil || r.From.Runtime >= s.Identity.Runtime {
			return bad
		}
		switch r.Phase {
		case "binding", "bound", "complete":
		default:
			return bad
		}
		if r.Transaction != nil {
			e := r.Transaction.Envelope
			if _, err := bridge.SlotPayload(e); err != nil {
				return bad
			}
			if e.Runtime != r.From.Runtime || e.Owner != s.Owner || e.GUID != r.From.GUID || e.Build != r.From.Build {
				return bad
			}
		}
	}
	if r := s.Reload; r != nil {
		if ValidateRequest(r.Request) != nil || r.InputStep < 0 || r.InputStep > 16 || r.PatchTransitions < 0 || r.PatchTransitions > 2 {
			return bad
		}
		if _, err := tokenBytes(r.From); err != nil {
			return bad
		}
		switch r.Phase {
		case "intent", "input_attempted", "binding", "complete":
		default:
			return bad
		}
		if r.Phase != "complete" && (s.Closed || s.Operation != nil && s.Operation.Stage != "complete" && (s.Operation.Stage != "prepared" || s.Operation.PreparedNonce != "") && !(s.Operation.Stage == "release_ready" && s.Operation.CleanupMethod == "reload_required")) {
			return bad
		}
	}
	if tx := s.Transaction; tx != nil {
		if tx.EscapeAttempts < 0 {
			return bad
		}
		e := tx.Envelope
		if _, err := bridge.SlotPayload(e); err != nil {
			return bad
		}
		if e.Owner != s.Owner || e.Runtime != s.Identity.Runtime || e.GUID != s.Identity.GUID || e.Build != s.Identity.Build {
			return bad
		}
		switch tx.Phase {
		case "intent", "published", "input_attempted":
			if tx.Receipt != nil {
				return bad
			}
		case "received":
			if tx.Receipt == nil || tx.Receipt.Nonce != e.Nonce || tx.Receipt.Ticket != e.Ticket || tx.Receipt.Action != e.Action {
				return bad
			}
		default:
			return bad
		}
	}
	if op := s.Operation; op != nil {
		switch op.CleanupMethod {
		case "":
		case "released", "runtime_destroyed":
			if op.Stage != "complete" {
				return bad
			}
		case "reload_required":
			if op.Stage != "release_ready" {
				return bad
			}
		default:
			return bad
		}
		if op.Attempt < 0 || op.Attempt > 3 {
			return bad
		}
		if op.Origin != nil && op.Origin.Validate() != nil {
			return bad
		}
		if op.Request != "" && ValidateRequest(op.Request) != nil {
			return bad
		}
		if !strings.HasPrefix(op.ID, "LMO-") {
			return bad
		}
		if _, err := tokenBytes(strings.TrimPrefix(op.ID, "LMO-")); err != nil {
			return bad
		}
		if _, err := tokenBytes(op.Ticket); err != nil {
			return bad
		}
		if len(op.Code) < 1 || len(op.Code) > 262144 || op.Budget < 1 || op.Budget > 120 || (op.Policy != "observation" && op.Policy != "opaque") {
			return bad
		}
		switch op.Stage {
		case "prepared", "execution_unknown":
		case "commit_ready", "running", "confirm_ready", "result_verified", "release_ready", "complete":
			if _, err := tokenBytes(op.PreparedNonce); err != nil {
				return bad
			}
			if _, err := tokenBytes(op.Challenge); err != nil {
				return bad
			}
		default:
			return bad
		}
		if op.Stage == "confirm_ready" || op.Stage == "result_verified" || op.Stage == "release_ready" || op.Stage == "complete" {
			if !json.Valid(op.Result) || len(op.Result) > bridge.MemoryMaxPayload || uint32(len(op.Result)) != op.ReportBytes || adler32.Checksum(op.Result) != op.ReportChecksum {
				return bad
			}
		}
	}
	return nil
}
