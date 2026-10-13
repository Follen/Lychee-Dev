//go:build windows && amd64

package command

import (
	"context"
	"testing"
)

func TestLiveDoctorRejectsMutatingFlagsWithoutFix(t *testing.T) {
	for _, args := range [][]string{
		{"--release", "candidate"}, {"--output", "archive"}, {"--request", "repair"},
		{"--session", "CON-11111111111111111111111111111111", "--pid", "42"},
	} {
		opts, err := parseOptions(append([]string{"live", "doctor"}, args...))
		if err != nil {
			t.Fatal(err)
		}
		_, code, err := runLiveDoctorCommand(context.Background(), opts, &Envelope{})
		if code != 2 || err == nil {
			t.Fatalf("flags reached target discovery: %v %d %v", args, code, err)
		}
	}
}

func TestDoctorUsesLiveChecksOnlyForAnExplicitTarget(t *testing.T) {
	if got := liveDoctorChecks(context.Background(), Options{}); len(got) != 0 {
		t.Fatal(got)
	}
	for _, command := range [][]string{{"doctor"}, {"live", "doctor"}} {
		opts, err := parseOptions(append(command, "--installation", "client", "--pid", "42", "--project", "workspace"))
		if err != nil || opts.pid != 42 || opts.installation != "client" || opts.project != "workspace" || opts.fix {
			t.Fatal(opts, err)
		}
	}
	opts, err := parseOptions([]string{"live", "doctor", "--fix", "--release", "candidate"})
	if err != nil || !opts.fix || opts.release != "candidate" {
		t.Fatal(opts, err)
	}
}
