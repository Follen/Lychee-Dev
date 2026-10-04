package duplex

// ValidateFrameTransition authorizes only a fresh whole-command publication.
// The addon repeats these checks against its private challenge and terminal.
func ValidateFrameTransition(s Sendbox, m Message) error {
	h := m.Header
	if h.Kind != Frame || h.FrameIndex != 1 || h.FrameCount != 1 || s.Runtime != h.Runtime || s.Arena != h.Arena || s.ActorBinding != h.ActorBinding {
		return ErrIdentity
	}
	if !s.Ready || !s.TransportReady || !s.ActorReady || s.ReadyChallenge == "" || s.ReadyChallenge == zeroToken {
		return ErrPending
	}
	if h.Challenge != s.ReadyChallenge {
		return ErrIdentity
	}
	id := Identity{Runtime: h.Runtime, Arena: h.Arena, Session: h.Session, Owner: h.Owner, ActorBinding: h.ActorBinding, Fence: h.Fence}
	if !exact(id, s) {
		expected, e := NextIdentity(s, h.Owner, h.Session)
		if e != nil || id != expected {
			return ErrIdentity
		}
	}
	switch s.Phase {
	case "ready_unbound":
		if s.Request != nil || s.Terminal != nil || h.RequestSeq != 1 || h.PreviousResultAckSHA != zeroDigest {
			return ErrIdentity
		}
	case "result_pending":
		if !exact(id, s) || s.Terminal == nil || !s.ResourcesReleased {
			return ErrPending
		}
		ack, e := ResultAckSHA256(*s.Terminal)
		if e != nil || h.PreviousResultAckSHA != ack {
			return ErrIdentity
		}
		if s.Request == nil || h.RequestSeq != s.Request.RequestSeq+1 || s.Request.RequestSeq == ^uint64(0) {
			return ErrIdentity
		}
	default:
		return ErrPending
	}
	return nil
}
