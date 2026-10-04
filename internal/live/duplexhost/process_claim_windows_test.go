//go:build windows && amd64

package duplexhost

import (
	"context"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/live"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/vault"
)

func mustWriterScope(t *testing.T, n *Native) string {
	t.Helper()
	scope, err := n.scope()
	if err != nil {
		t.Fatal(err)
	}
	return scope
}

func claimTarget(t *testing.T) (live.ClientWindow, journal.WindowOwner) {
	t.Helper()
	target := live.ClientWindow{}
	target.Client.Directory = t.TempDir()
	target.Window.ProcessID, target.Window.ProcessStartedAt, target.Window.Handle = 7, uint64(time.Now().UnixNano()), 10
	if err := os.MkdirAll(addonParent(target), 0700); err != nil {
		t.Fatal(err)
	}
	claim := journal.WindowOwner{Schema: "lycheedev.window-owner.v1", WorkspaceID: strings.Repeat("a", 32), Resource: fmt.Sprintf("window/7/%d/10", target.Window.ProcessStartedAt), OperationID: "CON-" + strings.Repeat("b", 32), IntentSHA256: strings.Repeat("c", 64)}
	scope, err := processScope(target)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = os.RemoveAll(scope) }) // unique owned fixture directory
	return target, claim
}

func TestProcessClaimAndRowLeaseIgnoreInstallationAliasesAndHWND(t *testing.T) {
	ctx := context.Background()
	target, claim := claimTarget(t)
	if err := beginConnectionClaim(ctx, target, claim, func() error { return nil }); err != nil {
		t.Fatal(err)
	}
	alias := target
	alias.Client.Directory = filepath.Join(t.TempDir(), "other-project-installation-alias")
	alias.Window.Handle++
	aliasClaim := claim
	aliasClaim.Resource = fmt.Sprintf("window/7/%d/%d", alias.Window.ProcessStartedAt, alias.Window.Handle)
	aliasClaim.WorkspaceID = strings.Repeat("d", 32)
	aliasClaim.OperationID = "CON-" + strings.Repeat("e", 32)
	called := false
	if err := beginConnectionClaim(ctx, alias, aliasClaim, func() error { called = true; return nil }); !errors.Is(err, journal.ErrBusy) || called {
		t.Fatal("alias bypassed instance claim", err, called)
	}
	one, two := &Native{Target: target}, &Native{Target: alias}
	if mustWriterScope(t, one) != mustWriterScope(t, two) {
		t.Fatal("path alias split writer scope")
	}
	lease, err := vault.TryAcquireLease(ctx, mustWriterScope(t, one), one.resource("command"))
	if err != nil {
		t.Fatal(err)
	}
	defer lease.Close()
	if other, err := vault.TryAcquireLease(ctx, mustWriterScope(t, two), two.resource("command")); err == nil {
		other.Close()
		t.Fatal("path alias bypassed row lock")
	}
	if err := verifyConnectionClaim(ctx, target, claim); err != nil {
		t.Fatal(err)
	}
	legacyDriver, err := journal.LockBootstrapWindow(ctx, addonParent(target), claim)
	if err != nil {
		t.Fatal(err)
	}
	if newDriver, err := lockConnectionDriver(ctx, target, claim); err == nil {
		newDriver.Close()
		t.Fatal("older CLI driver was bypassed")
	}
	legacyDriver.Close()
	driver, err := lockConnectionDriver(ctx, target, claim)
	if err != nil {
		t.Fatal(err)
	}
	if err := retireConnectionClaim(ctx, target, claim); err == nil {
		t.Fatal("active instance driver was retired")
	}
	if err := verifyConnectionClaim(ctx, target, claim); err != nil {
		t.Fatal("failed retirement removed compatibility marker", err)
	}
	driver.Close()
	lease.Close()
	legacyRow, err := vault.TryAcquireLease(ctx, filepath.Join(addonParent(target), ".lycheedev-duplex-writers"), one.resource("stop"))
	if err != nil {
		t.Fatal(err)
	}
	defer legacyRow.Close()
	if release, err := one.WritersDrained(ctx); err == nil {
		release()
		t.Fatal("older CLI row writer was bypassed")
	}
}

func TestProcessClaimPIDCreationIsolationAndLegacyFailClosed(t *testing.T) {
	ctx := context.Background()
	target, claim := claimTarget(t)
	if err := journal.BeginConnectionWindow(ctx, addonParent(target), claim, func() error { return nil }); err != nil {
		t.Fatal(err)
	}
	if _, _, err := inspectConnectionOwner(ctx, target); err == nil || !strings.Contains(err.Error(), "legacy_process_claim") {
		t.Fatal("legacy claim ignored", err)
	}
	if _, err := lockConnectionDriver(ctx, target, claim); err == nil {
		t.Fatal("legacy connection adopted without process claim")
	}
	if err := journal.RetireConnectionWindow(ctx, addonParent(target), claim); err != nil {
		t.Fatal(err)
	}
	if err := beginConnectionClaim(ctx, target, claim, func() error { return nil }); err != nil {
		t.Fatal(err)
	}
	for _, mode := range []string{"other_pid", "pid_reused"} {
		fresh := target
		if mode == "other_pid" {
			fresh.Window.ProcessID++
		} else {
			fresh.Window.ProcessStartedAt++
		}
		freshClaim := claim
		freshClaim.Resource = fmt.Sprintf("window/%d/%d/10", fresh.Window.ProcessID, fresh.Window.ProcessStartedAt)
		if err := beginConnectionClaim(ctx, fresh, freshClaim, func() error { return nil }); err != nil {
			t.Fatal("distinct instance blocked", mode, err)
		}
		if err := verifyConnectionClaim(ctx, fresh, claim); err == nil {
			t.Fatal("old instance claim authorized replacement", mode)
		}
		if err := retireConnectionClaim(ctx, fresh, freshClaim); err != nil {
			t.Fatal(err)
		}
		scope, _ := processScope(fresh)
		if err := os.RemoveAll(scope); err != nil {
			t.Fatal(err)
		}
	}
}
