//go:build windows && amd64

package channel

import (
	"context"
	"crypto/sha256"
	"fmt"
	"hash/adler32"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/live"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/selection"
)

func projectFixture(t *testing.T) (*Project, *Driver, projectTarget, string) {
	t.Helper()
	p, err := OpenProject(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	i := Identity{Runtime: strings.Repeat("1", 32), NextSlot: 2, Slots: 64, GUID: "g", Character: "c", Realm: "r", Build: "b", Product: "retail", Release: "2.5.1"}
	d, err := New("", nil, i)
	if err != nil {
		t.Fatal(err)
	}
	d.Log = p.log(d.State.ID)
	d.State.Bound = true
	target := live.ClientWindow{Client: selection.ClientInstallation{Directory: t.TempDir()}}
	parent := filepath.Join(target.Client.Directory, "Interface", "AddOns")
	if err := os.MkdirAll(parent, 0700); err != nil {
		t.Fatal(err)
	}
	hash := sha256.Sum256([]byte(d.State.ID))
	owner := journal.WindowOwner{Schema: "lycheedev.window-owner.v1", WorkspaceID: p.workspaceID(), Resource: "window/123/456/789", OperationID: d.State.ID, IntentSHA256: fmt.Sprintf("%x", hash)}
	meta := projectTarget{Schema: "lycheedev.channel-target.v1", Target: target, Owner: owner}
	err = journal.BeginConnectionWindow(context.Background(), parent, owner, func() error {
		if e := writeProjectJSON(context.Background(), p.path("connections", d.State.ID+".target.json"), meta); e != nil {
			return e
		}
		return d.Save(context.Background(), "fixture_bound")
	})
	if err != nil {
		t.Fatal(err)
	}
	return p, d, meta, parent
}

func TestClosedProjectRetiresClaimAfterCrashWithoutGame(t *testing.T) {
	p, d, meta, parent := projectFixture(t)
	d.State.Bound = false
	d.State.Closed = true
	if err := d.Save(context.Background(), "connection_closed"); err != nil {
		t.Fatal(err)
	}
	// Deliberately skip host retirement to model a crash after the durable close.
	for n := 0; n < 2; n++ {
		r, err := p.Disconnect(context.Background(), d.State.ID, false)
		if err != nil || !r.Closed {
			t.Fatalf("%+v %v", r, err)
		}
	}
	if _, busy, err := journal.InspectWindowOwner(context.Background(), parent, meta.Owner.Resource); err != nil || busy {
		t.Fatalf("claim retained: %v %v", busy, err)
	}
}

func TestPendingCloseDoesNotInheritBoundCompletion(t *testing.T) {
	_, d, _, _ := projectFixture(t)
	d.State.Closing = true
	r := present(d)
	if r.Complete || r.Stage != "closing" || !r.Bound {
		t.Fatalf("%+v", r)
	}
}

func TestClosingUnknownOperationDoesNotInventCompletion(t *testing.T) {
	p, d, _, _ := projectFixture(t)
	ctx := context.Background()
	if err := d.PrepareRequest(ctx, "unknown", "return 1", 5, "opaque"); err != nil {
		t.Fatal(err)
	}
	d.State.Operation.Stage = "execution_unknown"
	d.State.Bound = false
	d.State.Closed = true
	if err := d.Save(ctx, "connection_closed"); err != nil {
		t.Fatal(err)
	}
	r, err := p.Disconnect(ctx, d.State.ID, false)
	if err != nil || !r.Closed || r.Complete || r.ReportState != "unavailable" || r.OperationState != "execution_unknown" {
		t.Fatalf("close invented business completion: %+v %v", r, err)
	}
}

func TestCompletedRequestRetryNeverTouchesGameOrReplacesLaterOperation(t *testing.T) {
	p, d, _, _ := projectFixture(t)
	ctx := context.Background()
	if err := d.PrepareRequest(ctx, "first", "return 1", 5, "opaque"); err != nil {
		t.Fatal(err)
	}
	op := d.State.Operation
	op.PreparedNonce = strings.Repeat("4", 32)
	op.Challenge = strings.Repeat("5", 32)
	op.Result = []byte(`{"ok":true}`)
	op.ReportBytes = uint32(len(op.Result))
	op.ReportChecksum = adler32.Checksum(op.Result)
	op.Stage = "complete"
	first := op.ID
	if err := d.Save(ctx, "first_complete"); err != nil {
		t.Fatal(err)
	}
	if err := d.PrepareRequest(ctx, "second", "return 2", 5, "opaque"); err != nil {
		t.Fatal(err)
	}
	second := d.State.Operation.ID
	r, err := p.Execute(ctx, d.State.ID, "first", "return 1", 5, "opaque", false)
	if err != nil || !r.Complete || r.Operation != first {
		t.Fatalf("%+v %v", r, err)
	}
	if _, err = p.Execute(ctx, d.State.ID, "first", "return 9", 5, "opaque", false); err == nil {
		t.Fatal("request key accepted changed code")
	}
	loaded, err := Load(d.Log, nil)
	if err != nil || loaded.State.Operation.ID != second {
		t.Fatal("historical retry changed current operation", err)
	}
}

func TestCopiedProjectCannotClaimOriginalConnection(t *testing.T) {
	p, d, meta, _ := projectFixture(t)
	other, err := OpenProject(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	if err = writeProjectJSON(context.Background(), other.path("connections", d.State.ID+".target.json"), meta); err != nil {
		t.Fatal(err)
	}
	if _, err = other.Status(d.State.ID); err == nil {
		t.Fatal("copied owner accepted", p.Root)
	}
}

func TestHistoricalReloadRetryDoesNotRepeatAfterLaterReload(t *testing.T) {
	p, d, _, _ := projectFixture(t)
	ctx := context.Background()
	d.State.Reload = &ReloadAttempt{Request: "first", From: strings.Repeat("0", 32), Phase: "complete", InputStep: 6}
	if err := d.Save(ctx, "first_reload_verified"); err != nil {
		t.Fatal(err)
	}
	d.State.Reload.Request = "second"
	if err := d.Save(ctx, "second_reload_verified"); err != nil {
		t.Fatal(err)
	}
	r, err := p.Reload(ctx, d.State.ID, "first", false)
	if err != nil || r.Reload == nil || r.Reload.Request != "first" {
		t.Fatalf("historical reload touched game: %+v %v", r, err)
	}
	current, err := Load(d.Log, nil)
	if err != nil || current.State.Reload.Request != "second" {
		t.Fatal("changed current state", err)
	}
}

func TestActivationStatusHasNoInventedRuntimeAndRejectsInvalidPhase(t *testing.T) {
	p, d, _, _ := projectFixture(t)
	ctx := context.Background()
	if err := os.Remove(d.Log); err != nil {
		t.Fatal(err)
	}
	a := activation{Schema: "lycheedev.channel-activation.v1", Request: "install", Phase: "input_attempted", InputStep: 6}
	if err := writeProjectJSON(ctx, p.activationPath(d.State.ID), a); err != nil {
		t.Fatal(err)
	}
	r, err := p.Status(d.State.ID)
	if err != nil || r.Complete || r.Bound || r.Identity.Runtime != "" || r.Stage != "activation_input_attempted" {
		t.Fatalf("%+v %v", r, err)
	}
	a.Phase = "pretend_ready"
	if err = writeProjectJSON(ctx, p.activationPath(d.State.ID), a); err != nil {
		t.Fatal(err)
	}
	if _, err = p.Status(d.State.ID); err == nil {
		t.Fatal("invalid activation accepted")
	}
}

func TestConnectionHistoryRotationPreservesOldRequestsAndRejectsMissingHistory(t *testing.T) {
	p, d, _, _ := projectFixture(t)
	ctx := context.Background()
	complete := func(key string) string {
		t.Helper()
		if err := d.PrepareRequest(ctx, key, "return 1", 5, "opaque"); err != nil {
			t.Fatal(err)
		}
		op := d.State.Operation
		op.PreparedNonce = strings.Repeat("4", 32)
		op.Challenge = strings.Repeat("5", 32)
		op.Result = []byte(`{"ok":true}`)
		op.ReportBytes = uint32(len(op.Result))
		op.ReportChecksum = adler32.Checksum(op.Result)
		op.Stage = "complete"
		if err := d.Save(ctx, "fixture_complete"); err != nil {
			t.Fatal(err)
		}
		return op.ID
	}
	first := complete("first")
	complete("second")
	for i := 0; i < 256; i++ {
		if err := d.Save(ctx, "fixture_observation"); err != nil {
			t.Fatal(err)
		}
	}
	if err := d.PrepareRequest(ctx, "third", "return 1", 5, "opaque"); err != nil {
		t.Fatal(err)
	}
	if d.State.Archive == "" {
		t.Fatal("history did not rotate")
	}
	events, err := journal.ReadMemoryLog(d.Log)
	if err != nil || len(events) != 2 {
		t.Fatalf("active segment: %d %v", len(events), err)
	}
	r, err := p.Execute(ctx, d.State.ID, "first", "return 1", 5, "opaque", false)
	if err != nil || r.Operation != first || !r.Complete {
		t.Fatalf("historical result: %+v %v", r, err)
	}
	archive := filepath.Join(d.Log+".history", d.State.Archive+".jsonl")
	if err = os.Rename(archive, archive+".missing"); err != nil {
		t.Fatal(err)
	}
	if _, err = p.Execute(ctx, d.State.ID, "first", "return 1", 5, "opaque", false); err == nil {
		t.Fatal("lost history allowed replay")
	}
	loaded, err := Load(d.Log, nil)
	if err != nil || loaded.State.Operation.Request != "third" {
		t.Fatal("historical lookup modified current request", err)
	}
}
