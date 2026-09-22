package command

import (
	"os"
	"path/filepath"
	"testing"
)

func TestLiveRunValidatesBeforeNativeWork(t *testing.T) {
	file := filepath.Join(t.TempDir(), "probe.lua")
	if err := os.WriteFile(file, []byte("return 42"), 0600); err != nil {
		t.Fatal(err)
	}
	base := []string{"live", "run", "--session", "SESSION-missing", "--file", file}
	for index := 2; index < len(base); index += 2 {
		args := append([]string{}, base[:index]...)
		args = append(args, base[index+2:]...)
		if result, code := invoke(t, append(args, "--format=json")...); code != 2 || result.OK || result.OperationID != "" {
			t.Fatalf("missing %s: %+v %d", base[index], result, code)
		}
	}
	for _, flag := range []string{"--installation=other", "--pid=7", "--character=Other", "--realm=Other", "--nonce=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "--snapshot=PIN-other", "--region=window", "--capture-area=window"} {
		if _, err := parseOptions(append(append([]string{}, base...), flag)); err == nil {
			t.Fatal("run accepted a connection override", flag)
		}
	}
	if result, code := invoke(t, append(append([]string{}, base...), "--account=../other", "--format=json")...); code != 2 || result.OK || result.OperationID != "" {
		t.Fatal("unsafe explicit account reached native work", result, code)
	}
	for _, body := range [][]byte{nil, make([]byte, (256<<10)+1)} {
		if err := os.WriteFile(file, body, 0600); err != nil {
			t.Fatal(err)
		}
		if result, code := invoke(t, append(base, "--format=json")...); code != 2 || result.OK {
			t.Fatalf("invalid code: %+v %d", result, code)
		}
	}
	if err := os.WriteFile(file, []byte("return 42"), 0600); err != nil {
		t.Fatal(err)
	}
	missing := filepath.Join(t.TempDir(), "absent")
	args := append(append([]string{}, base...), "--home", missing, "--format=json")
	if result, code := invoke(t, args...); code == 0 || result.OK || result.OperationID != "" {
		t.Fatalf("missing workspace: %+v %d", result, code)
	}
	if _, err := os.Stat(missing); !os.IsNotExist(err) {
		t.Fatal("run initialized workspace", err)
	}
	if result, code := invoke(t, "version", "--account", "ACCOUNT", "--format=json"); code != 2 || result.OK {
		t.Fatal("ignored account flag")
	}
}
