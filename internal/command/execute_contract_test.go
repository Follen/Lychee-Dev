package command

import (
	"errors"
	"github.com/follenfang/lycheedev/internal/live"
	"testing"
)

func TestPassiveDiscoveryParsesWithoutDowngradingToActive(t *testing.T) {
	for _, args := range [][]string{
		{"live", "instances", "--passive"},
		{"--passive", "live", "instances", "--installation", "client"},
	} {
		opts, err := parseOptions(args)
		if err != nil || !opts.passive {
			t.Fatalf("passive request lost: %+v %v", opts, err)
		}
	}
	for _, args := range [][]string{
		{"live", "connect", "--passive"},
		{"live", "instances", "--passive=false"},
		{"live", "instances", "--passive", "--passive"},
	} {
		if _, err := parseOptions(args); err == nil {
			t.Fatalf("accepted ambiguous passive flag: %v", args)
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
	for _, entry := range []struct {
		err  error
		exit int
	}{{live.ErrBusinessFailed, 5}, {live.ErrInvestigationPending, 6}, {live.ErrReportWriterAmbiguous, 3}} {
		exit, _, ok := queryFault(errors.Join(errors.New("detail"), entry.err))
		if !ok || exit != entry.exit {
			t.Fatal(entry, exit, ok)
		}
	}
}
