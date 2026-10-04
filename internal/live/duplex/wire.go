// Package duplex implements the versioned inbox/sendbox wire and durable
// host coordinator. It has no dependency on a particular memory layout.
package duplex

import (
	"bytes"
	"crypto/rand"
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"errors"
	"fmt"
)

const (
	HeaderBytes     = 320
	FrameBytes      = 4096
	MaxSourceBytes  = 1048576
	MaxFrames       = 256
	MaxControlBytes = 1024
)

type Kind uint32

const (
	Bind Kind = iota + 1
	Frame
	Commit
	Cancel
	Close
	ResultAck
	Reload
	Lease
	Repair
)

func (k Kind) Lane() string {
	switch k {
	case Bind, Repair:
		return "bindResume"
	case Frame:
		return "request"
	case Commit:
		return "commit"
	case Cancel:
		return "cancel"
	case Close:
		return "close"
	case ResultAck:
		return "resultAck"
	case Reload:
		return "reload"
	case Lease:
		return "lease"
	default:
		return ""
	}
}

type Identity struct {
	Runtime      string `json:"runtimeToken"`
	Arena        string `json:"arenaGeneration"`
	Session      string `json:"sessionToken"`
	Owner        string `json:"ownerToken"`
	ActorBinding string `json:"actorBindingId"`
	Fence        uint64 `json:"fence,string"`
}

type Header struct {
	Kind             Kind   `json:"kind"`
	Runtime          string `json:"runtimeToken"`
	Arena            string `json:"arenaGeneration"`
	Session          string `json:"sessionToken"`
	Owner            string `json:"ownerToken"`
	ActorBinding     string `json:"actorBindingId"`
	RequestID        string `json:"requestId"`
	MessageID        string `json:"messageId"`
	Challenge        string `json:"challenge"`
	Fence            uint64 `json:"fence,string"`
	PublicationSeq   uint64 `json:"publicationSeq,string"`
	RequestSeq       uint64 `json:"requestSeq,string"`
	TransportAttempt uint64 `json:"transportAttempt,string"`
	CreatedUTCMillis uint64 `json:"createdUtcMillis,string"`
	BudgetMillis     uint32 `json:"budgetMillis"`
	PayloadBytes     uint32 `json:"payloadBytes"`
	TotalBytes       uint32 `json:"totalBytes"`
	FrameIndex       uint32 `json:"frameIndex"`
	FrameCount       uint32 `json:"frameCount"`
	RequestSHA256    string `json:"requestSHA256"`
	FrameSHA256      string `json:"frameSHA256"`
	HeaderSHA256     string `json:"headerSHA256"`
	PublicationBegin uint64 `json:"publicationBegin,string"`
	PublicationEnd   uint64 `json:"publicationEnd,string"`
}

type Message struct {
	Header  Header `json:"header"`
	Payload []byte `json:"payload"`
}

func NewToken() (string, error) {
	var b [16]byte
	if _, err := rand.Read(b[:]); err != nil {
		return "", err
	}
	return hex.EncodeToString(b[:]), nil
}
func token(s string) ([]byte, error) {
	if len(s) != 32 {
		return nil, errors.New("token must be 32 lowercase hex characters")
	}
	b, e := hex.DecodeString(s)
	if e != nil || hex.EncodeToString(b) != s {
		return nil, errors.New("invalid token")
	}
	return b, nil
}
func digest(s string) ([]byte, error) {
	if len(s) != 64 {
		return nil, errors.New("digest must be 64 lowercase hex characters")
	}
	b, e := hex.DecodeString(s)
	if e != nil || hex.EncodeToString(b) != s {
		return nil, errors.New("invalid digest")
	}
	return b, nil
}
func hash(domain string, parts ...[]byte) string {
	h := sha256.New()
	h.Write([]byte(domain))
	for _, p := range parts {
		h.Write(p)
	}
	return hex.EncodeToString(h.Sum(nil))
}

func RequestDigest(h Header, source []byte) (string, error) {
	r, e := token(h.RequestID)
	if e != nil {
		return "", e
	}
	a, e := token(h.ActorBinding)
	if e != nil {
		return "", e
	}
	if len(source) > MaxSourceBytes || uint32(len(source)) != h.TotalBytes {
		return "", errors.New("invalid source length")
	}
	var n [16]byte
	binary.LittleEndian.PutUint32(n[:4], h.BudgetMillis)
	binary.LittleEndian.PutUint64(n[4:12], h.CreatedUTCMillis)
	binary.LittleEndian.PutUint32(n[12:], h.TotalBytes)
	return hash("LYCMBX/request/v1\x00", r, a, n[:], source), nil
}

func validateShape(h Header, payload []byte) error {
	if h.Kind.Lane() == "" {
		return errors.New("unknown message kind")
	}
	if h.PublicationSeq == 0 || h.PublicationBegin == 0 || h.PublicationBegin != h.PublicationEnd || h.PublicationBegin&1 != 0 {
		return errors.New("unstable publication")
	}
	if h.Fence == 0 || h.TransportAttempt == 0 {
		return errors.New("zero fence or transport attempt")
	}
	if h.PayloadBytes != uint32(len(payload)) {
		return errors.New("payload length mismatch")
	}
	if h.TotalBytes > MaxSourceBytes {
		return errors.New("source exceeds capacity")
	}
	if h.Kind == Frame {
		count := (h.TotalBytes + FrameBytes - 1) / FrameBytes
		if count == 0 {
			count = 1
		}
		if h.FrameCount != count || h.FrameIndex == 0 || h.FrameIndex > count || h.RequestSeq == 0 {
			return errors.New("invalid frame shape")
		}
		expected := uint32(FrameBytes)
		if h.FrameIndex == count {
			expected = h.TotalBytes - (count-1)*FrameBytes
		}
		if h.PayloadBytes != expected {
			return errors.New("invalid frame size")
		}
		if h.BudgetMillis == 0 || h.BudgetMillis > 120000 {
			return errors.New("business budget outside 1..120000 ms")
		}
	} else if len(payload) > MaxControlBytes || h.FrameIndex != 0 || h.FrameCount != 0 {
		return errors.New("invalid control shape")
	}
	return nil
}

// EncodeMessage computes the frame and header digests; digest fields supplied by
// the caller are ignored except for the immutable logical request digest.
func EncodeMessage(m Message) ([]byte, error) {
	h := m.Header
	h.PayloadBytes = uint32(len(m.Payload))
	if err := validateShape(h, m.Payload); err != nil {
		return nil, err
	}
	b := make([]byte, HeaderBytes+len(m.Payload))
	copy(b, "LYCMBX01")
	binary.LittleEndian.PutUint32(b[8:12], uint32(h.Kind))
	for i, s := range []string{h.Runtime, h.Arena, h.Session, h.Owner, h.ActorBinding, h.RequestID, h.MessageID, h.Challenge} {
		v, e := token(s)
		if e != nil {
			return nil, fmt.Errorf("token %d: %w", i, e)
		}
		copy(b[12+i*16:], v)
	}
	for i, n := range []uint64{h.Fence, h.PublicationSeq, h.RequestSeq, h.TransportAttempt, h.CreatedUTCMillis} {
		binary.LittleEndian.PutUint64(b[140+i*8:], n)
	}
	for i, n := range []uint32{h.BudgetMillis, h.PayloadBytes, h.TotalBytes, h.FrameIndex, h.FrameCount} {
		binary.LittleEndian.PutUint32(b[180+i*4:], n)
	}
	d, e := digest(h.RequestSHA256)
	if e != nil {
		return nil, e
	}
	copy(b[200:], d)
	fd := hash("LYCMBX/frame/v1\x00", b[:232], m.Payload)
	f, _ := hex.DecodeString(fd)
	copy(b[232:], f)
	hd := hash("LYCMBX/header/v1\x00", b[:HeaderBytes])
	v, _ := hex.DecodeString(hd)
	copy(b[264:], v)
	binary.LittleEndian.PutUint64(b[296:], h.PublicationBegin)
	binary.LittleEndian.PutUint64(b[304:], h.PublicationEnd)
	copy(b[HeaderBytes:], m.Payload)
	return b, nil
}

func DecodeMessage(b []byte) (Message, error) {
	var m Message
	if len(b) < HeaderBytes || string(b[:8]) != "LYCMBX01" {
		return m, errors.New("invalid wire header")
	}
	if !bytes.Equal(b[312:320], make([]byte, 8)) {
		return m, errors.New("nonzero reserved bytes")
	}
	h := &m.Header
	h.Kind = Kind(binary.LittleEndian.Uint32(b[8:12]))
	p := []*string{&h.Runtime, &h.Arena, &h.Session, &h.Owner, &h.ActorBinding, &h.RequestID, &h.MessageID, &h.Challenge}
	for i, s := range p {
		*s = hex.EncodeToString(b[12+i*16 : 28+i*16])
	}
	u := []*uint64{&h.Fence, &h.PublicationSeq, &h.RequestSeq, &h.TransportAttempt, &h.CreatedUTCMillis}
	for i, n := range u {
		*n = binary.LittleEndian.Uint64(b[140+i*8 : 148+i*8])
	}
	q := []*uint32{&h.BudgetMillis, &h.PayloadBytes, &h.TotalBytes, &h.FrameIndex, &h.FrameCount}
	for i, n := range q {
		*n = binary.LittleEndian.Uint32(b[180+i*4 : 184+i*4])
	}
	h.RequestSHA256 = hex.EncodeToString(b[200:232])
	h.FrameSHA256 = hex.EncodeToString(b[232:264])
	h.HeaderSHA256 = hex.EncodeToString(b[264:296])
	h.PublicationBegin = binary.LittleEndian.Uint64(b[296:304])
	h.PublicationEnd = binary.LittleEndian.Uint64(b[304:312])
	if len(b) != HeaderBytes+int(h.PayloadBytes) {
		return Message{}, errors.New("wire length mismatch")
	}
	m.Payload = append([]byte(nil), b[HeaderBytes:]...)
	if err := validateShape(*h, m.Payload); err != nil {
		return Message{}, err
	}
	if hash("LYCMBX/frame/v1\x00", b[:232], m.Payload) != h.FrameSHA256 {
		return Message{}, errors.New("frame SHA256 mismatch")
	}
	c := append([]byte(nil), b[:HeaderBytes]...)
	clear(c[264:312])
	if hash("LYCMBX/header/v1\x00", c) != h.HeaderSHA256 {
		return Message{}, errors.New("header SHA256 mismatch")
	}
	return m, nil
}

func NewFrames(id Identity, requestID string, requestSeq, attempt, created uint64, budget uint32, source []byte) ([]Message, error) {
	if len(source) > MaxSourceBytes {
		return nil, errors.New("source exceeds capacity")
	}
	h := Header{Kind: Frame, Runtime: id.Runtime, Arena: id.Arena, Session: id.Session, Owner: id.Owner, ActorBinding: id.ActorBinding, RequestID: requestID, Fence: id.Fence, RequestSeq: requestSeq, TransportAttempt: attempt, CreatedUTCMillis: created, BudgetMillis: budget, TotalBytes: uint32(len(source)), Challenge: "00000000000000000000000000000000"}
	d, e := RequestDigest(h, source)
	if e != nil {
		return nil, e
	}
	h.RequestSHA256 = d
	h.FrameCount = uint32((len(source) + FrameBytes - 1) / FrameBytes)
	if h.FrameCount == 0 {
		h.FrameCount = 1
	}
	frames := make([]Message, h.FrameCount)
	for i := range frames {
		start := i * FrameBytes
		end := min(start+FrameBytes, len(source))
		hh := h
		hh.FrameIndex = uint32(i + 1)
		hh.PublicationSeq = uint64(i + 1)
		hh.PublicationBegin = uint64(i+1) * 2
		hh.PublicationEnd = hh.PublicationBegin
		hh.MessageID, e = NewToken()
		if e != nil {
			return nil, e
		}
		hh.PayloadBytes = uint32(end - start)
		frames[i] = Message{hh, append([]byte(nil), source[start:end]...)}
		if _, e = EncodeMessage(frames[i]); e != nil {
			return nil, e
		}
	}
	return frames, nil
}
