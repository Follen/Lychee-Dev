package live

import (
	"context"
	"crypto/sha256"
	"encoding/json"
	"errors"
	"fmt"
	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/testkit"
	"hash/adler32"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"testing"
)

func TestReadInstalledReport(t *testing.T) {
	for _, identity := range []string{"flavor", "catalog"} {
		t.Run(identity, func(t *testing.T) { testReadInstalledReport(t, identity) })
	}
}

func testReadInstalledReport(t *testing.T, identity string) {
	client := testkit.Client(t, identity)
	directory := filepath.Join(client, "WTF", "Account", "Account-A", "SavedVariables")
	if err := os.MkdirAll(directory, 0700); err != nil {
		t.Fatal(err)
	}
	body, code := "42", []byte("return 42")
	signal := bridge.Signal{Schema: "lycheedev.signal.v1", Release: testkit.Version, Kind: "reported", SessionNonce: "session", RequestID: "request", Character: "Paladin", Realm: "Realm", Product: "retail", Build: "12.1.0.69875", Sequence: 2, CodeBytes: uint32(len(code)), CodeAdler32: fmt.Sprintf("%08x", adler32.Checksum(code)), ReportBytes: 2, ReportAdler32: fmt.Sprintf("%08x", adler32.Checksum([]byte(body)))}
	raw, _ := json.Marshal(signal)
	saved := []byte(fmt.Sprintf(`LycheeToolkitDB={schema=1,reports={request={receipt=%q,body=%q}}}`, raw, body))
	path := filepath.Join(directory, "Lychee Dev.lua")
	if err := os.WriteFile(path, saved, 0600); err != nil {
		t.Fatal(err)
	}
	expected := bridge.SignalExpectation{Release: signal.Release, Kind: signal.Kind, SessionNonce: signal.SessionNonce, RequestID: signal.RequestID, Character: signal.Character, Realm: signal.Realm, Product: signal.Product, Build: signal.Build, AfterSequence: 1}
	report, err := ReadInstalledReport(context.Background(), client, "Account-A", code, expected)
	if err != nil || report.Path != path || report.FileSHA256 != fmt.Sprintf("%x", sha256.Sum256(saved)) || string(report.Report.Body) != body {
		t.Fatalf("%+v %v", report, err)
	}
	for _, account := range []string{"", ".", "..", "../Account-A", "Account-A/../Account-A", "Account-A:stream", "Account-A.", "Account-A ", "Account-B", "bad\x00"} {
		if result, err := ReadInstalledReport(context.Background(), client, account, code, expected); err == nil || len(result.Report.Body) != 0 {
			t.Fatalf("account %q: %+v %v", account, result, err)
		}
	}
	wrong := expected
	wrong.Build = "12.1.0.99999"
	if _, err := ReadInstalledReport(context.Background(), client, "Account-A", code, wrong); err == nil {
		t.Fatal("wrong client accepted")
	}
	wrong = expected
	wrong.Character = "Other"
	if _, err := ReadInstalledReport(context.Background(), client, "Account-A", code, wrong); err == nil {
		t.Fatal("wrong character accepted")
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := ReadInstalledReport(ctx, client, "Account-A", code, expected); !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
	if err := os.Rename(path, path+".bak"); err != nil {
		t.Fatal(err)
	}
	if _, err := ReadInstalledReport(context.Background(), client, "Account-A", code, expected); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("backup used: %v", err)
	}
	for _, invalid := range []string{"", `LycheeDevDB={}`, `LycheeToolkitDB={schema=1,reports={}}`, string(saved) + " os.execute('bad')"} {
		if err := os.WriteFile(path, []byte(invalid), 0600); err != nil {
			t.Fatal(err)
		}
		if result, err := ReadInstalledReport(context.Background(), client, "Account-A", code, expected); err == nil || len(result.Report.Body) != 0 {
			t.Fatalf("invalid file accepted: %+v %v", result, err)
		}
	}
}

func TestReadInstalledReportRejectsRedirect(t *testing.T) {
	client := testkit.Client(t, "flavor")
	outside := t.TempDir()
	link := filepath.Join(client, "WTF")
	if runtime.GOOS == "windows" {
		command := exec.Command("powershell", "-NoProfile", "-NonInteractive", "-Command", "$ErrorActionPreference = 'Stop'; New-Item -ItemType Junction -Path $env:LYCHEEDEV_TEST_LINK -Target $env:LYCHEEDEV_TEST_TARGET | Out-Null")
		command.Env = append(os.Environ(), "LYCHEEDEV_TEST_LINK="+link, "LYCHEEDEV_TEST_TARGET="+outside)
		if output, err := command.CombinedOutput(); err != nil {
			t.Fatalf("junction: %v %s", err, output)
		}
	} else if err := os.Symlink(outside, link); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := os.Remove(link); err != nil {
			t.Error(err)
		}
	})
	expected := bridge.SignalExpectation{Product: "retail", Build: "12.1.0.69875"}
	if _, err := ReadInstalledReport(context.Background(), client, "Account-A", nil, expected); err == nil {
		t.Fatal("redirect accepted")
	}
}
