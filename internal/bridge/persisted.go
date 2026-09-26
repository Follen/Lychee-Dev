package bridge

import (
	"errors"
	"io"
)

// ReadPersistedReport selects one exact request from the new database, then
// verifies its original bytes against independently supplied operation identity.
// Reading or verifying does not acknowledge the game or delete any report.
//
// The stored receipt is the compact wire form, so the returned signal is filled
// from the caller's expectation before it is compared with a receipt that was
// read from the display, which the live reader already filled from its session
// baseline. Both sides of that comparison are then complete signals.
func ReadPersistedReport(reader io.Reader, code []byte, expected SignalExpectation) (VerifiedReport, error) {
	if expected.RequestID == "" {
		return VerifiedReport{}, errors.New("bridge.report_request_required")
	}
	state, err := ReadToolkitState(reader, SavedStateLimits())
	if err != nil {
		return VerifiedReport{}, err
	}
	if state["schema"] != float64(1) {
		return VerifiedReport{}, errors.New("bridge.unsupported_state_schema")
	}
	reports, ok := state["reports"].(LiteralTable)
	if !ok {
		return VerifiedReport{}, errors.New("bridge.invalid_report_store")
	}
	record, ok := reports[expected.RequestID].(LiteralTable)
	if !ok {
		return VerifiedReport{}, errors.New("bridge.report_missing")
	}
	receipt, receiptOK := record["receipt"].(string)
	body, bodyOK := record["body"].(string)
	if !receiptOK || !bodyOK {
		return VerifiedReport{}, errors.New("bridge.invalid_report_record")
	}
	report, err := VerifyReport([]byte(receipt), []byte(body), code, expected)
	if err != nil {
		return VerifiedReport{}, err
	}
	report.Receipt = FillSignalIdentity(report.Receipt, SignalIdentity{
		Release: expected.Release, Character: expected.Character, Realm: expected.Realm,
		Product: expected.Product, Build: expected.Build,
	})
	return report, nil
}

// VerifyPersistedReportRemoval verifies the selected original report and its
// absence from a complete subsequent database. It does not establish the origin
// or freshness of either reader, authorize deleting a queue, or prove cleaned.
// The live coordinator must bind the source and establish the post-ACK flush.
// Other requests may change concurrently; their contents are not compared.
func VerifyPersistedReportRemoval(before, after io.Reader, code []byte, expected SignalExpectation) (VerifiedReport, error) {
	report, err := ReadPersistedReport(before, code, expected)
	if err != nil {
		return VerifiedReport{}, err
	}
	return verifyReportAbsent(report, after, expected.RequestID)
}

// VerifyArchivedReportRemoval uses original archived bytes, not a reconstructed
// SavedVariables file. Source selection and post-ACK ordering belong to the host.
func VerifyArchivedReportRemoval(receipt, body []byte, after io.Reader, code []byte, expected SignalExpectation) (VerifiedReport, error) {
	report, err := VerifyReport(receipt, body, code, expected)
	if err != nil {
		return VerifiedReport{}, err
	}
	return verifyReportAbsent(report, after, expected.RequestID)
}

func verifyReportAbsent(report VerifiedReport, after io.Reader, requestID string) (VerifiedReport, error) {
	state, err := ReadToolkitState(after, SavedStateLimits())
	if err != nil {
		return VerifiedReport{}, err
	}
	if state["schema"] != float64(1) {
		return VerifiedReport{}, errors.New("bridge.unsupported_state_schema")
	}
	reports, ok := state["reports"].(LiteralTable)
	if !ok {
		return VerifiedReport{}, errors.New("bridge.invalid_report_store")
	}
	if _, exists := reports[requestID]; exists {
		return VerifiedReport{}, errors.New("bridge.report_still_present")
	}
	return report, nil
}
