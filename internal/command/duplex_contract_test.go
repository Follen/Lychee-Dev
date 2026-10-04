package command

import (
	"bytes"
	"os"
	"path/filepath"
	"testing"
)

func TestDuplexOnlyDirectoryRejectsRetiredInput(t *testing.T) {
	for _, path := range [][]string{{"live", "run"}, {"live", "bind"}, {"live", "reload", "fallback"}, {"live", "reset"}, {"live", "ack"}, {"live", "finish"}, {"live", "hide"}, {"live", "abandon"}, {"live", "probe", "load"}} {
		got, code := invoke(t, append(path, "--help", "--format=json")...)
		if code != 2 || got.OK {
			t.Fatalf("retired path advertised: %v %+v %d", path, got, code)
		}
	}
	for _, path := range [][]string{{"live", "status", "OP-old"}, {"live", "resume", "BTP-old"}, {"live", "cancel", "OP-old"}} {
		got, code := invoke(t, append(path, "--format=json")...)
		if code != 2 || got.OK {
			t.Fatalf("old execution identity accepted: %v %+v %d", path, got, code)
		}
	}
}

func TestDuplexProbeFileCapacity(t *testing.T) {
	path := filepath.Join(t.TempDir(), "probe.lua")
	source := bytes.Repeat([]byte(" "), 1<<20)
	if err := os.WriteFile(path, source, 0600); err != nil {
		t.Fatal(err)
	}
	got, err := readProbeFile(path)
	if err != nil || !bytes.Equal(got, source) {
		t.Fatal("1 MiB rejected", err)
	}
	if err := os.WriteFile(path, append(source, ' '), 0600); err != nil {
		t.Fatal(err)
	}
	if _, err := readProbeFile(path); err == nil {
		t.Fatal("capacity overflow accepted")
	}
}

func TestTargetDoctorHelpHasProjectAndTargetFlags(t *testing.T) {
	for _, args := range [][]string{
		{"doctor", "--project", t.TempDir(), "--session", "CON-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "--offline", "--help", "--format=json"},
		{"doctor", "--project", t.TempDir(), "--installation", "client", "--pid", "123", "--offline", "--help", "--format=json"},
	} {
		got, code := invoke(t, args...)
		if code != 0 || !got.OK {
			t.Fatal(got, code)
		}
	}
}
