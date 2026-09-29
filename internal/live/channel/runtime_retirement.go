package channel

import (
	"context"
	"errors"
)

func runtimeRetirementPending() error {
	return &BlockedError{Blocker: Blocker{Kind: "runtime_retirement_pending", Condition: "precise_resource_retirement"}}
}

// Retirement proves destruction only. It never replaces the original actor,
// grants a binding or turns an unconfirmed result into a verified report.
func retirementProofMatchesState(s State, proof *RuntimeReplacementProof) error {
	if proof == nil || proof.Validate(s.Identity) != nil {
		return errors.New("live.channel_runtime_retirement_invalid")
	}
	if r := s.Recovery; r != nil && r.From.Runtime == proof.Current.Runtime {
		return errors.New("live.channel_runtime_retirement_invalid")
	}
	for _, tx := range []*Transaction{s.Transaction, recoveryTransaction(s.Recovery)} {
		if tx != nil && (tx.Envelope.Runtime == proof.Current.Runtime || tx.Envelope.Build != proof.Current.Build) {
			return errors.New("live.channel_runtime_retirement_invalid")
		}
	}
	return nil
}

func recoveryTransaction(r *RuntimeRecovery) *Transaction {
	if r == nil {
		return nil
	}
	return r.Transaction
}

// Wrappers add context, but every underlying cause must still be an optional
// timeout. Never suppress an independent I/O, identity or cancellation fault.
func optionalObservationTimeout(err error) bool {
	if err == nil {
		return false
	}
	if multi, ok := err.(interface{ Unwrap() []error }); ok {
		children := multi.Unwrap()
		if len(children) == 0 {
			return false
		}
		for _, child := range children {
			if !optionalObservationTimeout(child) {
				return false
			}
		}
		return true
	}
	if single, ok := err.(interface{ Unwrap() error }); ok {
		return optionalObservationTimeout(single.Unwrap())
	}
	return err == ErrPending || err == context.DeadlineExceeded
}
