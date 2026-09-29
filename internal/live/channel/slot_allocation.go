package channel

// A suggestion is returned only before publication has changed any file. It
// retains the original dependency for callers which cannot reallocate (legacy
// runtimes, or transactions with any already-journaled input attempt).
type slotAvailable struct {
	Index   int
	Blocked *BlockedError
}

func (e *slotAvailable) Error() string { return e.Blocked.Error() }
func (e *slotAvailable) Unwrap() error { return e.Blocked }
