package live

import "testing"

func TestNextActionDoesNotLoopOnTerminalReportFailure(t *testing.T) {
	for _, test := range []struct {
		name    string
		outcome Outcome
		resume  bool
	}{
		{"report_unavailable", Outcome{Status: "failed", Cleanup: "pending", Report: ReportOutcome{State: "unavailable", ErrorCode: "report_store_limit"}}, false},
		{"verified_cleanup_pending", Outcome{Status: "unresolved", Cleanup: "pending", Report: ReportOutcome{State: "verified"}}, true},
		{"complete", Outcome{Status: "completed", Cleanup: "complete", Complete: true}, false},
		{"abandoned", Outcome{Status: "abandoned", Cleanup: "abandoned"}, false},
	} {
		t.Run(test.name, func(t *testing.T) {
			test.outcome.Goal = "finished"
			test.outcome.OperationID = "OP-original"
			got := withNextAction(test.outcome)
			if (got.NextAction != nil) != test.resume {
				t.Fatalf("invalid recovery advice: %+v", got)
			}
			if test.resume && (got.NextAction.Command != "lycheedev" || len(got.NextAction.Args) != 3 || got.NextAction.Args[2] != "OP-original") {
				t.Fatalf("changed recovery target: %+v", got.NextAction)
			}
		})
	}
}
