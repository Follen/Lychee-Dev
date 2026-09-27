package bridge

import "errors"

func parseCheckpoint(signal Signal) (Signal, error) {
	if !queueHex(signal.ProbeNonce, 32) || !queueHex(signal.SessionNonce, 32) || !queueHex(signal.CodeSHA256, 64) ||
		!queueKey(signal.RequestID) || signal.GUID == "" || signal.Character == "" || signal.Realm == "" || signal.RuntimeEpoch == 0 ||
		signal.InputReady || len(signal.Receipt) > 1400 || signal.ReportScope != "" || signal.ReceiverProtocol != "" ||
		signal.PriorSessionSequence != 0 || signal.PriorSessionEpoch != 0 || signal.WakeBinding != "" || signal.SubmitBinding != "" || signal.CloseBinding != "" {
		return signal, errors.New("bridge.invalid_checkpoint")
	}
	switch signal.WorkState {
	case "loaded", "running":
		if signal.Receipt != "" {
			return signal, errors.New("bridge.invalid_checkpoint_receipt")
		}
	case "reported", "report_error", "acknowledged", "finished":
		if signal.Receipt == "" {
			return signal, errors.New("bridge.checkpoint_receipt_missing")
		}
	default:
		return signal, errors.New("bridge.invalid_checkpoint_state")
	}
	// Reuse the normal session-field checks without treating a checkpoint as
	// readiness or allowing its nested immutable receipt to authorize input.
	plain := signal
	plain.Kind, plain.ProbeNonce = "ready", ""
	if _, err := parseSessionSignal(plain); err != nil {
		return signal, err
	}
	if signal.Receipt != "" {
		receipt, err := ParseSignal([]byte(signal.Receipt))
		if err != nil || receipt.RequestID != signal.RequestID {
			return signal, errors.New("bridge.checkpoint_receipt_mismatch")
		}
		want := signal.WorkState
		if want == "finished" {
			want = "acknowledged"
		}
		if receipt.Kind != want {
			return signal, errors.New("bridge.checkpoint_receipt_kind")
		}
		// Omitted compact identity may be filled from the outer checkpoint, but
		// an explicitly contradictory nested identity must never be accepted.
		for _, pair := range [][2]string{{receipt.SessionNonce, signal.SessionNonce}, {receipt.Character, signal.Character}, {receipt.Realm, signal.Realm}, {receipt.GUID, signal.GUID}, {receipt.Release, signal.Release}, {receipt.Product, signal.Product}, {receipt.Build, signal.Build}} {
			if pair[0] != "" && pair[0] != pair[1] {
				return signal, errors.New("bridge.checkpoint_receipt_identity")
			}
		}
	}
	return signal, nil
}
