package live

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
	"hash/adler32"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"testing"
)

func operationSessionFixture(t *testing.T, client string, input ReportIntent) (*WindowSession, *sessionFixtureFrames) {
	return operationSessionFixtureAtSequence(t, client, input, 1)
}

func operationSessionFixtureAtSequence(t *testing.T, client string, input ReportIntent, sequence uint64) (*WindowSession, *sessionFixtureFrames) {
	t.Helper()
	e := input.Expected
	ready := bridge.Signal{Schema: "lycheedev.signal.v1", Kind: "ready", Release: e.Release, SessionNonce: e.SessionNonce, Character: e.Character, Realm: e.Realm, GUID: input.Load.GUID, Product: e.Product, Build: e.Build, Sequence: sequence, InputReady: true, RuntimeEpoch: 1}
	frames := &sessionFixtureFrames{ackFrames: ackFrames{frame: makeAckFrame(t, ready)}}
	target := ClientWindow{Client: selection.ClientInstallation{Directory: client, Product: e.Product, FullBuild: e.Build}, Window: desktop.WindowIdentity{Handle: 1, ProcessID: 2, ProcessStartedAt: 3}}
	e.Kind, e.RequestID, e.RequireInputReady = "ready", "", true
	e.AfterSequence = 0
	session, err := observeWindowSession(context.Background(), target, e, frames, func(context.Context, ClientWindow) error { return nil })
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(session.Close)
	return session, frames
}

func TestSessionOperationRejectsInstallationRetarget(t *testing.T) {
	for _, when := range []int{1, 2} {
		t.Run(fmt.Sprint(when), func(t *testing.T) {
			ctx := context.Background()
			root, client, book, prepared, input := probeOperationFixture(t)
			link := filepath.Join(t.TempDir(), "installation")
			linkTo := func(target string) {
				if runtime.GOOS == "windows" {
					command := exec.Command("powershell", "-NoProfile", "-NonInteractive", "-Command", "$ErrorActionPreference = 'Stop'; New-Item -ItemType Junction -Path $env:LYCHEEDEV_TEST_LINK -Target $env:LYCHEEDEV_TEST_TARGET | Out-Null")
					command.Env = append(os.Environ(), "LYCHEEDEV_TEST_LINK="+link, "LYCHEEDEV_TEST_TARGET="+target)
					if output, err := command.CombinedOutput(); err != nil {
						t.Fatalf("junction: %v %s", err, output)
					}
				} else if err := os.Symlink(target, link); err != nil {
					t.Fatal(err)
				}
			}
			linkTo(client)
			t.Cleanup(func() {
				if err := os.Remove(link); err != nil {
					t.Error(err)
				}
			})
			input.Load.Installation = link
			raw, _ := json.Marshal(input)
			record, err := book.BeginWork(ctx, journal.WorkIntent{Kind: "probe", Resource: "window/retarget", Snapshot: prepared.Intent.Snapshot, Session: input.Expected.SessionNonce, Request: raw})
			if err != nil {
				t.Fatal(err)
			}
			// Isolate receipt observation from deployment's separate path rules.
			if err := book.AdvanceStage(ctx, journal.StageChange{OperationID: record.OperationID, ExpectedGeneration: record.Generation, ExpectedStage: "prepared", Stage: "load_requested", Status: "running", Observation: json.RawMessage(`{"schema":"lycheedev.probe-load.v1","revision":{"sha256":"0000000000000000000000000000000000000000000000000000000000000000","entries":1}}`)}); err != nil {
				t.Fatal(err)
			}
			session, frames := operationSessionFixture(t, client, input)
			foreign := t.TempDir()
			checks := 0
			session.confirm = func(context.Context, ClientWindow) error {
				checks++
				if checks == when {
					if err := os.Remove(link); err != nil {
						t.Fatal(err)
					}
					linkTo(foreign)
				}
				return nil
			}
			e := input.Expected
			loaded := bridge.Signal{Schema: "lycheedev.signal.v1", Kind: "loaded", Release: e.Release, SessionNonce: e.SessionNonce, RequestID: e.RequestID, ReloadNonce: input.Load.ReloadNonce, Character: e.Character, Realm: e.Realm, Product: e.Product, Build: e.Build, Sequence: 2, InputReady: true, CodeBytes: uint32(len(input.Code)), CodeAdler32: fmt.Sprintf("%08x", adler32.Checksum(input.Code))}
			frames.frame = makeAckFrame(t, loaded)
			frames.frame.SystemTicks = 2
			if capture, err := session.observeOperation(ctx, root, record.OperationID, nil); err == nil || err.Error() != "live.operation_installation_mismatch" || capture.ID != "" {
				t.Fatalf("retarget: %+v %v", capture, err)
			}
			current, err := book.InspectWork(ctx, record.OperationID)
			if err != nil || current.Stage != "load_requested" {
				t.Fatalf("retarget advanced: %+v %v", current, err)
			}
		})
	}
}

func TestSessionOperationRejectsForeignOrChangedIdentity(t *testing.T) {
	for _, mode := range []string{"nonce", "character", "realm", "guid", "release", "product", "build", "installation", "closed", "before-window", "after-window", "old-sequence", "ready-floor", "old-frame", "cancelled"} {
		t.Run(mode, func(t *testing.T) {
			ctx := context.Background()
			root, client, book, record, input := probeOperationFixture(t)
			if _, err := PrepareOperationQueue(ctx, root, record.OperationID); err != nil {
				t.Fatal(err)
			}
			session, frames := operationSessionFixture(t, client, input)
			e := input.Expected
			loaded := bridge.Signal{Schema: "lycheedev.signal.v1", Kind: "loaded", Release: e.Release, SessionNonce: e.SessionNonce, RequestID: e.RequestID, ReloadNonce: input.Load.ReloadNonce, Character: e.Character, Realm: e.Realm, Product: e.Product, Build: e.Build, Sequence: 2, InputReady: true, CodeBytes: uint32(len(input.Code)), CodeAdler32: fmt.Sprintf("%08x", adler32.Checksum(input.Code))}
			switch mode {
			case "nonce":
				session.ready.SessionNonce = "other"
			case "character":
				session.ready.Character = "Other"
			case "realm":
				session.ready.Realm = "Other"
			case "guid":
				session.ready.GUID = "Player-1-456"
			case "release":
				session.ready.Release = "other"
			case "product":
				session.ready.Product = "classic"
			case "build":
				session.ready.Build = "12.1.0.99999"
			case "installation":
				session.target.Client.Directory = t.TempDir()
			case "closed":
				session.Close()
			case "old-sequence":
				loaded.Sequence = 1
			case "ready-floor":
				session.ready.Sequence = 10
			case "cancelled":
				var cancel context.CancelFunc
				ctx, cancel = context.WithCancel(ctx)
				cancel()
			}
			checks := 0
			session.confirm = func(context.Context, ClientWindow) error {
				checks++
				if mode == "before-window" || mode == "after-window" && checks == 2 {
					return desktop.ErrIdentityChanged
				}
				return nil
			}
			frames.frame = makeAckFrame(t, loaded)
			if mode != "old-frame" {
				frames.frame.SystemTicks = 2
			}
			if capture, err := session.observeOperation(ctx, root, record.OperationID, nil); err == nil || capture.ID != "" {
				t.Fatalf("accepted %s: %+v %v", mode, capture, err)
			} else if (mode == "before-window" || mode == "after-window") && !errors.Is(err, desktop.ErrIdentityChanged) {
				t.Fatal(err)
			}
			current, err := book.InspectWork(context.Background(), record.OperationID)
			if err != nil || current.Stage != "load_requested" {
				t.Fatalf("rejected observation advanced work: %+v %v", current, err)
			}
			captures, err := vault.WriteMetadata(context.Background(), root, func(store *vault.Store, metadata *vault.Metadata) ([]evidence.CaptureRef, error) {
				return evidence.OpenArchive(store, metadata).ListCaptures(context.Background(), "", 10)
			})
			if err != nil || len(captures) != 1 || captures[0].Provenance.Kind != "decoded-window-session" {
				t.Fatalf("rejected observation left evidence: %+v %v", captures, err)
			}
		})
	}
}
