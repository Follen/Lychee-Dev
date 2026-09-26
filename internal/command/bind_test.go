package command

import (
	"github.com/follenfang/lycheedev/internal/desktop"
	"image"
	"os"
	"path/filepath"
	"testing"
)

func TestLiveBindArgumentsAndNoImplicitInitialization(t *testing.T) {
	for _, args := range [][]string{
		{}, {"--pid", "2"},
		{"--snapshot", "PIN-missing", "--pid", "0"},
		{"--snapshot", "PIN-missing", "--pid", "4294967296"},
		{"--snapshot", "PIN-missing", "--character", "bad\nname"},
		{"--snapshot", "PIN-missing", "--nonce", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},
		{"--snapshot", "PIN-missing", "--region", "window"},
	} {
		argv := append([]string{"live", "bind", "--format=json"}, args...)
		if result, code := invoke(t, argv...); code != 2 || result.OK {
			t.Fatalf("invalid %v: %+v code=%d", args, result, code)
		}
	}
	for _, area := range []string{"-1,0,1,1", "0,0,0,1", "0,0,4097,600", "16384,0,1,1", "0,1,2"} {
		if result, code := invoke(t, "live", "bind", "--snapshot", "PIN-missing", "--capture-area", area, "--format=json"); code != 2 || result.OK {
			t.Fatalf("invalid area %s: %+v code=%d", area, result, code)
		}
	}
	missing := filepath.Join(t.TempDir(), "absent-home")
	for _, filters := range [][]string{{}, {"--pid", "2"}, {"--installation", t.TempDir()}, {"--character", "Paladin", "--realm", "Realm"}} {
		argv := append([]string{"live", "bind", "--snapshot", "PIN-missing", "--home", missing, "--format=json"}, filters...)
		if result, code := invoke(t, argv...); code == 0 || code == 2 || result.OK || result.Result != nil {
			t.Fatalf("optional filters %v: %+v code=%d", filters, result, code)
		}
	}
	if _, err := os.Stat(missing); !os.IsNotExist(err) {
		t.Fatal("bind initialized workspace", err)
	}
	for _, flag := range []string{"--pid=2", "--character=Paladin", "--realm=Realm", "--capture-area=window"} {
		if result, code := invoke(t, "version", flag, "--format=json"); code != 2 || result.OK {
			t.Fatal("ignored binding flag", flag)
		}
	}
}

func TestCaptureAreaDefaultsToAutomaticRegion(t *testing.T) {
	for _, flags := range [][]string{{}, {"--capture-area", "window"}} {
		opts, err := parseOptions(append([]string{"live", "bind"}, flags...))
		want := image.Rectangle{}
		if len(flags) > 0 {
			want = desktop.WholeWindowCapture()
		}
		if err != nil || opts.region != want {
			t.Fatalf("whole window: %+v %v", opts, err)
		}
	}
	opts, err := parseOptions([]string{"live", "bind", "--capture-area", "1,2,600,500"})
	if err != nil || opts.region.Min.X != 1 || opts.region.Min.Y != 2 || opts.region.Dx() != 600 || opts.region.Dy() != 500 {
		t.Fatalf("custom area: %+v %v", opts, err)
	}
}
