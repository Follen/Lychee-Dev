//go:build windows && amd64

package command

import (
	"context"
	"testing"
)

func TestDoctorRejectsMutatingFlagsWithoutFix(t *testing.T) {
	for _, args := range [][]string{
		{"--release", "candidate"}, {"--output", "archive"}, {"--request", "repair"}, {"--hard-reset"},
		{"--session", "CON-11111111111111111111111111111111", "--pid", "42"},
	} {
		opts, err := parseOptions(append([]string{"doctor"}, args...))
		if err != nil {
			t.Fatal(err)
		}
		code, err := runDoctor(context.Background(), opts, &Envelope{})
		if code != 2 || err == nil {
			t.Fatalf("flags reached target discovery: %v %d %v", args, code, err)
		}
	}
}

func TestDoctorUsesLiveChecksOnlyForAnExplicitTarget(t *testing.T) {
	checks, report, err := doctorLive(context.Background(), Options{}, &Envelope{})
	if len(checks) != 0 || report != nil || err != nil {
		t.Fatal(checks, report, err)
	}
	opts, err := parseOptions([]string{"doctor", "--installation", "client", "--pid", "42", "--project", "workspace"})
	if err != nil || opts.pid != 42 || opts.installation != "client" || opts.project != "workspace" || opts.fix {
		t.Fatal(opts, err)
	}
}
