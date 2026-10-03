package duplex

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"math"
	"os"
	"path/filepath"

	"github.com/follenfang/lycheedev/internal/vault"
)

type WriteState string

const (
	NoWrite       WriteState = "not_written"
	CompleteWrite WriteState = "written"
	PartialWrite  WriteState = "partial"
	UnknownWrite  WriteState = "unknown"
)

type WriteOutcome struct {
	State            WriteState `json:"state"`
	Bytes            uint64     `json:"bytes,string"`
	ReadbackVerified bool       `json:"readbackVerified"`
}
type Intent struct {
	Message   Message      `json:"message"`
	Outcome   WriteOutcome `json:"outcome"`
	Accepted  bool         `json:"accepted"`
	Challenge string       `json:"challenge,omitempty"`
	Rejected  string       `json:"rejected,omitempty"`
}
type ActiveRequest struct {
	RequestID               string          `json:"requestId"`
	Digest                  string          `json:"requestSHA256"`
	Sequence                uint64          `json:"requestSeq,string"`
	Attempt                 uint64          `json:"transportAttempt,string"`
	Created                 uint64          `json:"createdUtcMillis,string"`
	Budget                  uint32          `json:"budgetMillis"`
	TransferDeadline        uint64          `json:"transferDeadlineUtcMillis,string"`
	ExecutionObservedHostAt uint64          `json:"executionObservedHostAtUtcMillis,string"`
	PreviousCommit          *Intent         `json:"previousCommit,omitempty"`
	RepairProof             *RepairProof    `json:"repairProof,omitempty"`
	Source                  []byte          `json:"source"`
	Frames                  []Message       `json:"frames"`
	NextFrame               uint32          `json:"nextFrame"`
	Phase                   string          `json:"phase"`
	Result                  *ResultManifest `json:"result,omitempty"`
	ResultSaved             bool            `json:"resultSaved"`
	Released                bool            `json:"released"`
}
type State struct {
	Schema              string            `json:"schema"`
	Identity            Identity          `json:"identity"`
	Bound               bool              `json:"bound"`
	Closed              bool              `json:"closed"`
	RequestSequence     uint64            `json:"requestSequence,string"`
	PublicationSequence uint64            `json:"publicationSequence,string"`
	Active              *ActiveRequest    `json:"active,omitempty"`
	Intents             map[string]Intent `json:"intents"`
	RepairProof         *RepairProof      `json:"repairProof,omitempty"`
}

// Update is an atomic, durable read-modify-write across all host processes.
// Its callback must not perform transport I/O. SaveResult returns only after
// the exact result bytes are durable; it must be idempotent for a manifest.
type Store interface {
	Load(context.Context) (State, error)
	Update(context.Context, func(*State) error) error
	SaveResult(context.Context, ResultManifest, []byte) error
}

// FileStore's directory is private host evidence. OS leases cover only short
// journal transactions and release automatically when the owner exits.
type FileStore struct{ Dir string }

func NewFileStore(dir string) *FileStore { return &FileStore{Dir: dir} }
func (s *FileStore) lock(ctx context.Context) (func(), error) {
	lease, e := vault.AcquireLease(ctx, filepath.Join(s.Dir, "locks"), "duplex-journal")
	if e != nil {
		return nil, e
	}
	return func() { _ = lease.Close() }, nil
}
func (s *FileStore) load() (State, error) {
	var st State
	p, e := readBounded(filepath.Join(s.Dir, "state.json"), 4*MaxSourceBytes)
	if os.IsNotExist(e) {
		return State{Schema: "lycheedev.duplex.journal.v1", Intents: map[string]Intent{}}, nil
	}
	if e != nil {
		return st, e
	}
	if len(p) > 4*MaxSourceBytes {
		return st, errors.New("journal capacity exceeded")
	}
	if e = json.Unmarshal(p, &st); e != nil {
		return st, e
	}
	if st.Schema != "lycheedev.duplex.journal.v1" || st.Intents == nil {
		return st, errors.New("invalid journal schema")
	}
	return st, ValidateState(st)
}
func (s *FileStore) Load(ctx context.Context) (State, error) {
	unlock, e := s.lock(ctx)
	if e != nil {
		return State{}, e
	}
	defer unlock()
	return s.load()
}
func (s *FileStore) Update(ctx context.Context, f func(*State) error) error {
	unlock, e := s.lock(ctx)
	if e != nil {
		return e
	}
	defer unlock()
	st, e := s.load()
	if e != nil {
		return e
	}
	if e = f(&st); e != nil {
		return e
	}
	if e = ValidateState(st); e != nil {
		return e
	}
	p, e := json.Marshal(st)
	if e != nil {
		return e
	}
	if len(p) > 4*MaxSourceBytes {
		return errors.New("journal capacity exceeded")
	}
	if e = vault.ReplaceFile(ctx, filepath.Join(s.Dir, "state.json"), p); e != nil {
		return errors.Join(ErrPersistence, e)
	}
	return nil
}
func (s *FileStore) SaveResult(ctx context.Context, m ResultManifest, p []byte) error {
	if e := ValidateResult(m, p); e != nil {
		return e
	}
	unlock, e := s.lock(ctx)
	if e != nil {
		return e
	}
	defer unlock()
	if _, e = token(m.RequestID); e != nil {
		return e
	}
	path := filepath.Join(s.Dir, "result-"+m.RequestID+".bin")
	old, e := readBounded(path, MaxResultBytes)
	if e == nil {
		return ValidateResult(m, old)
	}
	if !os.IsNotExist(e) {
		return e
	}
	if e = vault.ReplaceFile(ctx, path, p); e != nil {
		return errors.Join(ErrPersistence, e)
	}
	return nil
}
func readBounded(path string, limit int64) ([]byte, error) {
	f, e := os.Open(path)
	if e != nil {
		return nil, e
	}
	defer f.Close()
	info, e := f.Stat()
	if e != nil {
		return nil, e
	}
	if !info.Mode().IsRegular() || info.Size() > limit {
		return nil, errors.New("evidence file exceeds bounds or is not regular")
	}
	p, e := io.ReadAll(io.LimitReader(f, limit+1))
	if e == nil && int64(len(p)) > limit {
		return nil, errors.New("evidence file grew beyond bounds")
	}
	return p, e
}

// ValidateState checks durable protocol facts without reading files, acquiring
// leases or changing state. Read-only inspection uses the same validation as
// journal transactions before presenting evidence as verified.
func ValidateState(st State) error {
	if st.Schema != "lycheedev.duplex.journal.v1" || st.Intents == nil || len(st.Intents) > 8 {
		return errors.New("invalid durable journal")
	}
	if st.Identity != (Identity{}) {
		for _, v := range []string{st.Identity.Runtime, st.Identity.Arena, st.Identity.Session, st.Identity.Owner, st.Identity.ActorBinding} {
			if _, e := token(v); e != nil {
				return e
			}
		}
		if st.Identity.Fence == 0 {
			return errors.New("invalid journal fence")
		}
	} else if st.Bound || st.Closed || st.Active != nil || len(st.Intents) != 0 || st.RepairProof != nil || st.RequestSequence != 0 || st.PublicationSequence != 0 {
		return errors.New("durable facts require an identity")
	}
	for lane, in := range st.Intents {
		if lane != in.Message.Header.Kind.Lane() {
			return errors.New("journal lane mismatch")
		}
		if _, e := EncodeMessage(in.Message); e != nil {
			return e
		}
		if in.Accepted && in.Rejected != "" {
			return errors.New("contradictory durable receipt")
		}
		if in.Challenge != "" {
			if _, e := token(in.Challenge); e != nil {
				return e
			}
		}
		switch in.Outcome.State {
		case NoWrite, CompleteWrite, PartialWrite, UnknownWrite:
		default:
			return errors.New("invalid journal outcome")
		}
	}
	a := st.Active
	if a == nil {
		return nil
	}
	if !st.Bound {
		return errors.New("durable request requires binding")
	}
	if _, e := token(a.RequestID); e != nil {
		return e
	}
	if _, e := digest(a.Digest); e != nil {
		return e
	}
	if a.Sequence == 0 || a.Attempt == 0 || a.Sequence > st.RequestSequence || a.Budget == 0 || a.Budget > 120000 || a.Created > math.MaxInt64-600000 || a.TransferDeadline < a.Created || a.TransferDeadline > a.Created+600000 {
		return errors.New("invalid durable request")
	}
	if a.Result != nil {
		if e := validateResultManifest(*a.Result); e != nil {
			return e
		}
		if a.Result.RequestID != a.RequestID || a.Result.RequestSHA256 != a.Digest {
			return errors.New("durable result identity mismatch")
		}
	}
	if a.Released {
		if !a.ResultSaved || a.Result == nil || len(a.Source) != 0 || len(a.Frames) != 0 {
			return errors.New("invalid released journal")
		}
		return nil
	}
	if len(a.Source) > MaxSourceBytes || len(a.Frames) == 0 || len(a.Frames) > MaxFrames || a.NextFrame > uint32(len(a.Frames)) {
		return errors.New("invalid durable source")
	}
	first := a.Frames[0].Header
	d, e := RequestDigest(first, a.Source)
	if e != nil || d != a.Digest || first.RequestID != a.RequestID || first.RequestSeq != a.Sequence || first.TransportAttempt != a.Attempt || first.BudgetMillis != a.Budget || first.CreatedUTCMillis != a.Created {
		return errors.New("durable logical digest mismatch")
	}
	for i, m := range a.Frames {
		if m.Header.Kind != Frame || m.Header.RequestID != a.RequestID || m.Header.RequestSHA256 != a.Digest || m.Header.FrameIndex != uint32(i+1) || m.Header.FrameCount != uint32(len(a.Frames)) || m.Header.Runtime != st.Identity.Runtime || m.Header.Arena != st.Identity.Arena || m.Header.Session != st.Identity.Session || m.Header.Owner != st.Identity.Owner || m.Header.ActorBinding != st.Identity.ActorBinding || m.Header.Fence != st.Identity.Fence || m.Header.TransportAttempt != a.Attempt || m.Header.RequestSeq != a.Sequence || m.Header.CreatedUTCMillis != a.Created || m.Header.BudgetMillis != a.Budget {
			return errors.New("invalid durable frame")
		}
		start := i * FrameBytes
		end := min(start+FrameBytes, len(a.Source))
		if start > len(a.Source) || !bytes.Equal(m.Payload, a.Source[start:end]) {
			return errors.New("durable source/frame mismatch")
		}
		if _, e = EncodeMessage(m); e != nil {
			return e
		}
	}
	if a.ResultSaved && (a.Result == nil || a.Result.RequestID != a.RequestID || a.Result.RequestSHA256 != a.Digest) {
		return errors.New("invalid durable result manifest")
	}
	return nil
}
