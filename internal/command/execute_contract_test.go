package command

import (
	"reflect"
	"testing"
)

func TestPassiveDiscoveryParsesWithoutDowngradingToActive(t *testing.T) {
	for _, args := range [][]string{
		{"live", "instances", "--passive"},
		{"--passive", "live", "instances", "--installation", "client"},
	} {
		var implicit []string
		for _, arg := range args {
			if arg != "--passive" {
				implicit = append(implicit, arg)
			}
		}
		plain, err := parseOptions(implicit)
		if err != nil {
			t.Fatal(err)
		}
		opts, err := parseOptions(args)
		if err != nil || !reflect.DeepEqual(opts, plain) {
			t.Fatalf("explicit passive flag changed discovery: got=%+v want=%+v err=%v", opts, plain, err)
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

func TestNoCacheCompatibilityFlagPreservesNativeOptions(t *testing.T) {
	for _, args := range [][]string{
		{"--no-cache", "doctor", "--fix", "--installation", "client", "--pid", "42"},
		{"live", "connect", "--session", "CON-x", "--wait-seconds", "10", "--no-cache"},
		{"live", "execute", "--session", "CON-x", "--request", "sample", "--file", "probe.lua", "--budget-seconds", "5", "--policy", "observation", "--no-cache"},
		{"live", "disconnect", "CON-x", "--no-cache"},
		{"live", "resume", "CON-x", "--no-cache"},
		{"live", "reload", "--session", "CON-x", "--request", "sample", "--no-cache"},
		{"live", "reload", "fallback", "--installation", "client", "--pid", "42", "--request", "sample", "--no-cache"},
		{"live", "bugs", "--session", "CON-x", "--request", "sample", "--count", "2", "--no-cache"},
	} {
		var implicit []string
		for _, arg := range args {
			if arg != "--no-cache" {
				implicit = append(implicit, arg)
			}
		}
		plain, err := parseOptions(implicit)
		if err != nil {
			t.Fatal(err)
		}
		opts, err := parseOptions(args)
		if err != nil || !reflect.DeepEqual(opts, plain) {
			t.Fatalf("compatibility flag changed native options for %v: got=%+v want=%+v err=%v", args, opts, plain, err)
		}
	}
	for _, args := range [][]string{
		{"live", "instances", "--no-cache"},
		{"live", "status", "CON-x", "--no-cache"},
		{"live", "session", "CON-x", "--no-cache"},
		{"live", "connect", "--no-cache=false"},
		{"live", "connect", "--no-cache", "--no-cache"},
	} {
		if _, err := parseOptions(args); err == nil {
			t.Fatalf("accepted unsupported or ambiguous compatibility flag: %v", args)
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
