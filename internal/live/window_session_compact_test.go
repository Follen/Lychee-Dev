package live

import (
	"context"
	"testing"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/desktop"
)

func TestWindowSessionCompactReportUsesAdoptedNonce(t *testing.T) {
	_, client, _, _, input := probeOperationFixture(t)
	session, frames := operationSessionFixture(t, client, input)
	e := input.Expected
	e.Kind, e.AfterSequence = "reported", session.Ready().Sequence
	signal := bridge.Signal{Schema: "lycheedev.signal.v1", Kind: "reported", Release: e.Release,
		Product: e.Product, Build: e.Build, RequestID: e.RequestID, Sequence: e.AfterSequence + 1,
		ReportBytes: 2, ReportAdler32: "017500f9", InputReady: true}
	frames.frame = makeAckFrame(t, signal)
	frames.frame.SystemTicks = 2
	got, err := session.reader.WaitForSignal(context.Background(), e)
	if err != nil {
		t.Fatalf("compact reported receipt rejected: %v", err)
	}
	if got.SessionNonce != e.SessionNonce || got.Character != e.Character || got.Realm != e.Realm || got.GUID != session.Ready().GUID {
		t.Fatalf("lost established identity: %+v", got)
	}
}

func TestConnectAndReconnectCompactReady(t *testing.T) {
	client, fake, root := connectFixture(t)
	fake.windows = []desktop.WindowIdentity{testWindow(1, client)}
	fake.respond[1] = func(command string) []*desktop.CapturedFrame {
		if command == "/dev connect" {
			signal := readyReceipt(func(s *bridge.Signal) {
				s.Character, s.Realm, s.GUID = "", "", ""
				s.InputReady = true
			})
			return []*desktop.CapturedFrame{fake.frame(1, signal), fake.frame(1, signal)}
		}
		return identityResponder(fake, 1, nil)(command)
	}
	connection, err := connectWindow(context.Background(), root, ConnectRequest{Snapshot: testPin(t, root)}, fake.io())
	if err != nil {
		t.Fatal(err)
	}
	session, _, err := reconnectSession(context.Background(), root, connection.ID, fake.io())
	if err != nil {
		t.Fatal(err)
	}
	defer session.Close()
	if session.ready.Character != "Paladin" || session.ready.Realm != "Realm" || session.ready.GUID != "Player-1-123" {
		t.Fatalf("lost proved actor: %+v", session.ready)
	}
}
