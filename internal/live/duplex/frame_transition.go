package duplex

// ValidateFrameTransition protects reuse of the single physical command row.
// A host write/readback is not a consumer acknowledgement: only the matching
// private receive ledger permits the following logical frame.
func ValidateFrameTransition(s Sendbox, m Message) error {
	h := m.Header
	if h.Kind != Frame || s.Identity != (Identity{Runtime: h.Runtime, Arena: h.Arena, Session: h.Session, Owner: h.Owner, ActorBinding: h.ActorBinding, Fence: h.Fence}) {
		return ErrIdentity
	}
	if !s.TransportReady || !s.ActorReady {
		return ErrPending
	}
	if s.Request == nil {
		if !s.BusinessReady || (s.Phase != "idle" && s.Phase != "released") || h.FrameIndex != 1 {
			return ErrPending
		}
		return nil
	}
	r := s.Request
	if s.Phase != "receiving" || !r.NotStarted {
		return ErrPending
	}
	if r.RequestID != h.RequestID || r.RequestSHA256 != h.RequestSHA256 || r.RequestSeq != h.RequestSeq || r.TransportAttempt != h.TransportAttempt {
		return ErrIdentity
	}
	for i, n := range r.AcceptedFrames {
		if n != uint32(i+1) {
			return ErrIdentity
		}
	}
	if h.FrameIndex != uint32(len(r.AcceptedFrames)+1) {
		return ErrPending
	}
	return nil
}
