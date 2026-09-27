//go:build windows && amd64 && lycheedev_input_lab

package main

import "testing"

func TestReceiptCannotConfuseProductionOrMalformedInput(t *testing.T) {
	good := "LDIL1|0123456789abcdef|69933|abcd0123|1|ready|abcd012300000001|00000001|0|0|1"
	s, ok := parseSignal(good)
	if !ok || !s.Focus || s.State != "ready" {
		t.Fatalf("%+v %v", s, ok)
	}
	for _, bad := range []string{"lycheedev.signal.v1", good + "|extra", "LDIL1|x|b|e|-1|ready|n|h|0|0|1", "LDIL1|x|b|e|1|ready|n|h|999|0|1"} {
		if _, ok := parseSignal(bad); ok {
			t.Fatal("accepted", bad)
		}
	}
}
