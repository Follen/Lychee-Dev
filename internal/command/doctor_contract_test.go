package command

import "testing"

func TestDoctorOwnsRepairContract(t *testing.T) {
	opts, err := parseOptions([]string{"doctor", "--fix", "--hard-reset", "--installation", "client", "--pid", "42"})
	if err != nil || !opts.fix || !opts.hardReset || opts.pid != 42 {
		t.Fatalf("unified doctor repair contract: %+v %v", opts, err)
	}
	for _, c := range commandContracts {
		if c.Definition.Path == "live doctor" {
			t.Fatal("duplicate live doctor entry remains")
		}
	}
}
