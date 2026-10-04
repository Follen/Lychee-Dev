package duplex

import (
	"bytes"
	"encoding/binary"
	"errors"
	"math"
)

const ClosedBindProofBytes = 88

func releasedEqual(a, b *Released) bool {
	return a == nil && b == nil || a != nil && b != nil && *a == *b
}

// NextIdentity is admission for a fresh host claim. An existing owner can be
// replaced only after explicit close, resource quiescence and no unacknowledged
// request/result. The retained owner is fenced rather than silently cleared.
func NextIdentity(s Sendbox, owner, session string) (Identity, error) {
	if e := validateSendbox(s); e != nil {
		return Identity{}, e
	}
	for _, v := range []string{owner, session} {
		if _, e := token(v); e != nil {
			return Identity{}, e
		}
		if v == "00000000000000000000000000000000" {
			return Identity{}, ErrIdentity
		}
	}
	if !s.ControlReady || !s.ActorReady {
		return Identity{}, ErrPending
	}
	if s.Fence == math.MaxUint64 {
		return Identity{}, errors.New("owner fence exhausted")
	}
	if s.Owner != "00000000000000000000000000000000" {
		if _, e := EncodeClosedBindProof(s); e != nil {
			return Identity{}, e
		}
		if owner == s.Owner || session == s.Session {
			return Identity{}, ErrIdentity
		}
	} else if s.Session != "00000000000000000000000000000000" || s.Fence != 0 || s.Request != nil || s.Terminal != nil {
		return Identity{}, ErrIdentity
	}
	id := s.Identity
	id.Owner = owner
	id.Session = session
	id.Fence = s.Fence + 1
	return id, nil
}

// EncodeClosedBindProof binds the next session to the exact retained closed
// owner, session, fence and final released tombstone. It grants no execution.
func EncodeClosedBindProof(s Sendbox) ([]byte, error) {
	if s.Phase != "closed" || !s.ResourcesReleased || s.Request != nil || s.Terminal != nil || s.Owner == "00000000000000000000000000000000" || s.Session == "00000000000000000000000000000000" || s.Fence == 0 {
		return nil, ErrBusy
	}
	o, e := token(s.Owner)
	if e != nil {
		return nil, e
	}
	session, e := token(s.Session)
	if e != nil {
		return nil, e
	}
	p := make([]byte, ClosedBindProofBytes)
	copy(p, o)
	copy(p[16:], session)
	binary.LittleEndian.PutUint64(p[32:40], s.Fence)
	if s.Released != nil {
		r, e := token(s.Released.RequestID)
		if e != nil {
			return nil, e
		}
		d, e := digest(s.Released.RequestSHA256)
		if e != nil {
			return nil, e
		}
		copy(p[40:], r)
		copy(p[56:], d)
	}
	return p, nil
}

// ValidateBindTransition is shared by the coordinator and the native writer;
// the addon independently checks its private ledger before changing ownership.
func ValidateBindTransition(s Sendbox, m Message) error {
	h := m.Header
	id := Identity{h.Runtime, h.Arena, h.Session, h.Owner, h.ActorBinding, h.Fence}
	if h.Kind != Bind || h.Runtime != s.Runtime || h.Arena != s.Arena || h.ActorBinding != s.ActorBinding || !s.ControlReady || !s.ActorReady {
		return ErrIdentity
	}
	if h.RequestID != "00000000000000000000000000000000" || h.RequestSHA256 != "0000000000000000000000000000000000000000000000000000000000000000" || h.RequestSeq != 0 || h.BudgetMillis != 0 || h.TotalBytes != 0 {
		return ErrIdentity
	}
	if exact(id, s) {
		if s.Phase == "closed" || len(m.Payload) != 0 {
			return ErrBusy
		}
		return nil
	}
	expected, e := NextIdentity(s, h.Owner, h.Session)
	if e != nil {
		return e
	}
	if id != expected {
		return ErrIdentity
	}
	if s.Owner == "00000000000000000000000000000000" {
		if len(m.Payload) != 0 {
			return ErrIdentity
		}
		return nil
	}
	proof, e := EncodeClosedBindProof(s)
	if e != nil {
		return e
	}
	if !bytes.Equal(m.Payload, proof) {
		return ErrIdentity
	}
	return nil
}
