package channel

// Blocker reports a dependency observed while holding its admission lock. It
// identifies evidence to reconcile, not permission to take another owner over.
// No probe code, actor data or unrelated reservation payload is exposed.
type Blocker struct {
	Kind         string `json:"kind"`
	Installation string `json:"installation,omitempty"`
	Slot         int    `json:"slot,omitempty"`
	Consumer     string `json:"consumer,omitempty"`
	Runtime      string `json:"runtime,omitempty"`
	Nonce        string `json:"nonce,omitempty"`
	Condition    string `json:"condition,omitempty"`
}

type BlockedError struct {
	Blocker Blocker
}

func (e *BlockedError) Error() string {
	return "live.channel_blocked: " + e.Blocker.Kind
}

func (e *BlockedError) Unwrap() error {
	switch e.Blocker.Kind {
	case "slot_reservation", "slot_pool_full", "publication_lock":
		return ErrPublicationPending
	default:
		return ErrPending
	}
}
