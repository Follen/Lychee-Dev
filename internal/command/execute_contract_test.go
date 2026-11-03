package command

import (
	"testing"
)

func TestRetiredNoOpFlagsAreRejected(t *testing.T) {
	for _, args := range [][]string{{"live", "instances", "--passive"}, {"doctor", "--no-cache"}, {"live", "connect", "--no-cache"}} {
		if _, err := parseOptions(args); err == nil {
			t.Fatalf("retired flag accepted: %v", args)
		}
	}
}

func TestExecuteContractRejectsIncompleteRequestsBeforeWorkspaceAccess(t *testing.T) {
	for _, args := range [][]string{
		{}, {"--probe", "sample"},
		{"--probe", "sample", "--file", "missing.lua", "--session", "SES-x", "--request", "x", "--budget-seconds", "10"},
		{"--probe", "sample", "--session", "SES-x", "--request", "x", "--budget-seconds", "121"},
	} {
		response, exit := invoke(t, append(append([]string{"live", "execute"}, args...), "--format=json")...)
		if exit != 2 || response.OK || response.OperationID != "" {
			t.Fatalf("%v: %d %+v", args, exit, response)
		}
	}
	response, exit := invoke(t, "live", "execute", "--help", "--format=json")
	if exit != 0 || !response.OK {
		t.Fatal(response, exit)
	}
}
