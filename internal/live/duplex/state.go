package duplex

import (
	"bytes"
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
)

const MaxResultBytes = 524288
const MaxSendboxBytes = 65536

type RequestState struct {
	RequestID        string   `json:"requestId"`
	RequestSHA256    string   `json:"requestSHA256"`
	RequestSeq       uint64   `json:"requestSeq,string"`
	TransportAttempt uint64   `json:"transportAttempt,string"`
	TotalBytes       uint32   `json:"totalBytes"`
	AcceptedFrames   []uint32 `json:"acceptedFrames"`
	Challenge        string   `json:"challenge"`
	NotStarted       bool     `json:"notStarted"`
}
type Receipt struct {
	MessageID        string `json:"messageId"`
	RequestID        string `json:"requestId"`
	RequestSHA256    string `json:"requestSHA256"`
	State            string `json:"state"`
	Challenge        string `json:"challenge,omitempty"`
	ExecutionStarted bool   `json:"executionStarted"`
}
type ResultManifest struct {
	RequestID         string   `json:"requestId"`
	RequestSHA256     string   `json:"requestSHA256"`
	State             string   `json:"state"`
	SHA256            string   `json:"sha256"`
	Bytes             uint32   `json:"bytes"`
	Pages             uint32   `json:"pages"`
	PageSHA256        []string `json:"pageSHA256"`
	ExecutionStarted  bool     `json:"executionStarted"`
	Effects           string   `json:"effects"`
	ResourcesReleased bool     `json:"resourcesReleased"`
	FailureCode       string   `json:"failureCode,omitempty"`
}
type Released struct {
	RequestID     string `json:"requestId"`
	RequestSHA256 string `json:"requestSHA256"`
}

type ReloadPreparation struct {
	MessageID string `json:"messageId"`
	Challenge string `json:"challenge"`
}
type RepairProof struct {
	PreviousArena         string `json:"previousArena"`
	NewArena              string `json:"newArena"`
	Challenge             string `json:"challenge"`
	RequestID             string `json:"requestId"`
	RequestSHA256         string `json:"requestSHA256"`
	NotStarted            bool   `json:"notStarted"`
	LedgerRetained        bool   `json:"ledgerRetained"`
	PreviousWriterDrained bool   `json:"previousWriterDrained"`
	Idle                  bool   `json:"idle,omitempty"`
	NoPendingRequest      bool   `json:"noPendingRequest,omitempty"`
	ResourcesReleased     bool   `json:"resourcesReleased,omitempty"`
}
type ValidationProgress struct {
	RequestID     string `json:"requestId"`
	RequestSHA256 string `json:"requestSHA256"`
	TotalBytes    uint32 `json:"totalBytes"`
	CopiedBytes   uint32 `json:"copiedBytes"`
	HashedBytes   uint32 `json:"hashedBytes"`
}
type Sendbox struct {
	Validation      *ValidationProgress `json:"validation,omitempty"`
	ClosedAdmission bool                `json:"closedAdmission"`
	Identity
	ActorGUID         string             `json:"actorGUID"`
	ActorReady        bool               `json:"actorReady"`
	Character         string             `json:"character"`
	Realm             string             `json:"realm"`
	Build             string             `json:"build"`
	Product           string             `json:"product"`
	Release           string             `json:"release"`
	ResourcesReleased bool               `json:"resourcesReleased"`
	Schema            string             `json:"schema"`
	LayoutID          string             `json:"layoutId"`
	Phase             string             `json:"phase"`
	Ready             bool               `json:"ready"`
	TransportReady    bool               `json:"transportReady"`
	BusinessReady     bool               `json:"businessReady"`
	ControlReady      bool               `json:"controlReady"`
	ReadyChallenge    string             `json:"readyChallenge"`
	AdmissionSequence uint64             `json:"admissionSequence,string"`
	StatusSequence    uint64             `json:"statusSequence,string"`
	Heartbeat         uint64             `json:"heartbeat,string"`
	Request           *RequestState      `json:"request,omitempty"`
	Receipts          map[string]Receipt `json:"receipts"`
	Terminal          *ResultManifest    `json:"terminal,omitempty"`
	Released          *Released          `json:"released,omitempty"`
	Repair            *RepairProof       `json:"repair,omitempty"`
	Reload            *ReloadPreparation `json:"reload,omitempty"`
}

func validateSendbox(s Sendbox) error {
	if s.ClosedAdmission && ((s.Phase != "ready_unbound" && s.Phase != "validating") || !s.ResourcesReleased || s.Request != nil || s.Terminal != nil || s.Owner == zeroToken || s.Session == zeroToken || s.Fence == 0) {
		return errors.New("invalid closed admission proof")
	}
	if s.Ready != s.BusinessReady {
		return errors.New("ready does not match businessReady")
	}
	if s.BusinessReady && !s.ActorReady {
		return errors.New("businessReady requires current actorReady")
	}
	if s.ReadyChallenge != "" {
		if _, err := token(s.ReadyChallenge); err != nil || s.AdmissionSequence == 0 {
			return errors.New("invalid ready challenge")
		}
	}
	if s.BusinessReady && (s.ReadyChallenge == "" || s.ReadyChallenge == "00000000000000000000000000000000") {
		return errors.New("businessReady requires a one-use challenge")
	}
	if s.Schema != "lycheedev.mailbox.v1" {
		return errors.New("incompatible sendbox schema")
	}
	if s.LayoutID != MailboxLayoutID {
		return errors.New("incompatible mailbox layout")
	}
	for _, v := range []string{s.Runtime, s.Arena, s.Session, s.Owner, s.ActorBinding} {
		if _, e := token(v); e != nil {
			return e
		}
	}
	if s.StatusSequence == 0 || s.Heartbeat == 0 {
		return errors.New("missing sendbox sequence")
	}
	switch s.Phase {
	case "ready_unbound", "validating", "running", "result_pending", "closing", "closed", "quarantined", "execution_unknown":
	default:
		return errors.New("invalid sendbox phase")
	}
	if v := s.Validation; v != nil {
		if s.Phase != "validating" || s.Ready || v.TotalBytes > MaxSourceBytes || v.CopiedBytes > v.TotalBytes || v.HashedBytes > v.TotalBytes || v.HashedBytes > v.CopiedBytes {
			return errors.New("invalid validation progress")
		}
		if _, e := token(v.RequestID); e != nil {
			return e
		}
		if _, e := digest(v.RequestSHA256); e != nil {
			return e
		}
	} else if s.Phase == "validating" {
		return errors.New("validating phase requires progress")
	}
	if len(s.Receipts) > 1 {
		return errors.New("control receipt bound exceeded")
	}
	for lane, r := range s.Receipts {
		if lane != "stop" {
			return errors.New("unknown receipt lane")
		}
		if _, e := token(r.MessageID); e != nil {
			return e
		}
		if r.Challenge != "" {
			if _, e := token(r.Challenge); e != nil {
				return e
			}
		}
	}
	if s.Request != nil {
		r := s.Request
		if _, e := token(r.RequestID); e != nil {
			return e
		}
		if _, e := digest(r.RequestSHA256); e != nil {
			return e
		}
		if r.RequestSeq == 0 || r.TransportAttempt == 0 || r.TotalBytes > MaxSourceBytes || len(r.AcceptedFrames) > MaxFrames {
			return errors.New("invalid request projection")
		}
		seen := map[uint32]bool{}
		for _, i := range r.AcceptedFrames {
			if i == 0 || i > MaxFrames || seen[i] {
				return errors.New("invalid accepted frame bitmap")
			}
			seen[i] = true
		}
		if r.Challenge != "" {
			if _, e := token(r.Challenge); e != nil {
				return e
			}
		}
	}
	if s.Terminal != nil {
		r := s.Terminal
		if r.ExecutionStarted && r.Effects != "may_have_occurred" || !r.ExecutionStarted && r.Effects != "none_started" {
			return errors.New("invalid terminal execution/effects proof")
		}
		if _, e := token(r.RequestID); e != nil {
			return e
		}
		if _, e := digest(r.RequestSHA256); e != nil {
			return e
		}
		if _, e := digest(r.SHA256); e != nil {
			return e
		}
		if r.State != "success" && r.State != "failed" && r.State != "cancelled" {
			return errors.New("invalid terminal state")
		}
		count := (r.Bytes + 16383) / 16384
		if count == 0 {
			count = 1
		}
		if r.Bytes > MaxResultBytes || r.Pages != count {
			return errors.New("invalid result shape")
		}
		if len(r.PageSHA256) != int(r.Pages) {
			return errors.New("invalid result page digests")
		}
		for _, d := range r.PageSHA256 {
			if _, e := digest(d); e != nil {
				return e
			}
		}
	}
	if s.Released != nil {
		if _, e := token(s.Released.RequestID); e != nil {
			return e
		}
		if _, e := digest(s.Released.RequestSHA256); e != nil {
			return e
		}
	}
	if p := s.Repair; p != nil {
		for _, v := range []string{p.PreviousArena, p.NewArena, p.Challenge, p.RequestID} {
			if _, e := token(v); e != nil {
				return e
			}
		}
		if _, e := digest(p.RequestSHA256); e != nil {
			return e
		}
		if p.NewArena != s.Arena || p.PreviousArena == p.NewArena {
			return errors.New("invalid repair arena proof")
		}
		if p.Idle && (!p.NoPendingRequest || !p.ResourcesReleased || p.NotStarted || p.RequestID != "00000000000000000000000000000000" || p.RequestSHA256 != "0000000000000000000000000000000000000000000000000000000000000000") {
			return errors.New("invalid idle repair proof")
		}
	}
	if r := s.Reload; r != nil {
		if _, err := token(r.MessageID); err != nil || r.MessageID == "00000000000000000000000000000000" {
			return errors.New("invalid reload preparation message")
		}
		if _, err := token(r.Challenge); err != nil || r.Challenge == "00000000000000000000000000000000" {
			return errors.New("invalid reload preparation challenge")
		}
		if s.Request != nil || s.Terminal != nil || !s.ResourcesReleased || s.BusinessReady {
			return errors.New("reload preparation is not quiescent")
		}
	}
	return nil
}

func EncodeSendbox(s Sendbox) ([]byte, error) {
	if e := validateSendbox(s); e != nil {
		return nil, e
	}
	p, e := json.Marshal(s)
	if e != nil {
		return nil, e
	}
	if len(p) > MaxSendboxBytes {
		return nil, errors.New("sendbox capacity exceeded")
	}
	b := make([]byte, 44+len(p))
	copy(b, "LYCMSB01")
	binary.LittleEndian.PutUint32(b[8:12], uint32(len(p)))
	d := sha256.Sum256(p)
	copy(b[12:44], d[:])
	copy(b[44:], p)
	return b, nil
}

// DecodeSendbox accepts exactly one bounded, digest-verified JSON object.
func DecodeSendbox(b []byte) (Sendbox, error) {
	var s Sendbox
	if len(b) < 44 || string(b[:8]) != "LYCMSB01" || len(b) > 44+MaxSendboxBytes || int(binary.LittleEndian.Uint32(b[8:12])) != len(b)-44 {
		return s, errors.New("invalid sendbox envelope")
	}
	d := sha256.Sum256(b[44:])
	if !bytes.Equal(d[:], b[12:44]) {
		return s, errors.New("sendbox SHA256 mismatch")
	}
	var fields map[string]json.RawMessage
	if e := json.Unmarshal(b[44:], &fields); e != nil {
		return s, e
	}
	if _, ok := fields["ready"]; !ok {
		return s, errors.New("explicit sendbox ready field is required")
	}
	if _, ok := fields["actorReady"]; !ok {
		return s, errors.New("explicit sendbox actorReady field is required")
	}
	dec := json.NewDecoder(bytes.NewReader(b[44:]))
	dec.DisallowUnknownFields()
	if e := dec.Decode(&s); e != nil {
		return s, e
	}
	if e := dec.Decode(new(any)); e != io.EOF {
		return s, errors.New("trailing sendbox JSON")
	}
	return s, validateSendbox(s)
}

func ValidateResult(m ResultManifest, p []byte) error {
	if e := validateResultManifest(m); e != nil {
		return e
	}
	if len(p) > MaxResultBytes || uint32(len(p)) != m.Bytes {
		return errors.New("result length mismatch")
	}
	h := sha256.Sum256(p)
	if hex.EncodeToString(h[:]) != m.SHA256 {
		return fmt.Errorf("result SHA256 mismatch")
	}
	if len(m.PageSHA256) != int(m.Pages) {
		return errors.New("result page digest count mismatch")
	}
	for i, d := range m.PageSHA256 {
		start := i * 16384
		end := min(start+16384, len(p))
		if start > len(p) {
			return errors.New("result page outside body")
		}
		h := sha256.Sum256(p[start:end])
		if hex.EncodeToString(h[:]) != d {
			return errors.New("result page SHA256 mismatch")
		}
	}
	return nil
}

// ACK is fixed-width binary so the addon needs no JSON parser.
func EncodeResultAck(m ResultManifest) ([]byte, error) {
	if e := validateResultCore(m); e != nil {
		return nil, e
	}
	b := make([]byte, 92)
	r, e := token(m.RequestID)
	if e != nil {
		return nil, e
	}
	d, e := digest(m.RequestSHA256)
	if e != nil {
		return nil, e
	}
	h, e := digest(m.SHA256)
	if e != nil {
		return nil, e
	}
	var state uint32
	switch m.State {
	case "success":
		state = 1
	case "failed":
		state = 2
	case "cancelled":
		state = 3
	default:
		return nil, errors.New("invalid ACK terminal")
	}
	copy(b, r)
	copy(b[16:], d)
	binary.LittleEndian.PutUint32(b[48:], state)
	copy(b[52:], h)
	binary.LittleEndian.PutUint32(b[84:], m.Bytes)
	binary.LittleEndian.PutUint32(b[88:], m.Pages)
	return b, nil
}

// ResultAckSHA256 binds the exact fixed-width ACK to the following command.
func ResultAckSHA256(m ResultManifest) (string, error) {
	b, err := EncodeResultAck(m)
	if err != nil {
		return "", err
	}
	return hash("LYCMBX/result-ack/v1\x00", b), nil
}
func DecodeResultAck(b []byte) (ResultManifest, error) {
	var m ResultManifest
	if len(b) != 92 {
		return m, errors.New("invalid ACK length")
	}
	m.RequestID = hex.EncodeToString(b[:16])
	m.RequestSHA256 = hex.EncodeToString(b[16:48])
	switch binary.LittleEndian.Uint32(b[48:52]) {
	case 1:
		m.State = "success"
	case 2:
		m.State = "failed"
	case 3:
		m.State = "cancelled"
	default:
		return m, errors.New("invalid ACK terminal")
	}
	m.SHA256 = hex.EncodeToString(b[52:84])
	m.Bytes = binary.LittleEndian.Uint32(b[84:88])
	m.Pages = binary.LittleEndian.Uint32(b[88:92])
	return m, validateResultCore(m)
}

func validateResultCore(m ResultManifest) error {
	if _, e := token(m.RequestID); e != nil {
		return e
	}
	if _, e := digest(m.RequestSHA256); e != nil {
		return e
	}
	if _, e := digest(m.SHA256); e != nil {
		return e
	}
	if m.State != "success" && m.State != "failed" && m.State != "cancelled" {
		return errors.New("invalid result terminal")
	}
	count := (m.Bytes + 16383) / 16384
	if count == 0 {
		count = 1
	}
	if m.Bytes > MaxResultBytes || m.Pages != count {
		return errors.New("invalid result manifest shape")
	}
	return nil
}
func validateResultManifest(m ResultManifest) error {
	if e := validateResultCore(m); e != nil {
		return e
	}
	if len(m.PageSHA256) != int(m.Pages) {
		return errors.New("invalid result page digests")
	}
	for _, d := range m.PageSHA256 {
		if _, e := digest(d); e != nil {
			return e
		}
	}
	return nil
}
