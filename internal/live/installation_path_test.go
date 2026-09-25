package live

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/desktop"
)

func TestConnectInstallationSpellingPreservesSelectionAndSession(t *testing.T) {
	for _, spelling := range []string{"slash", "trailing-separator", "dot", "relative", "case"} {
		t.Run(spelling, func(t *testing.T) {
			client, fake, root := connectFixture(t)
			other := makeTestClient(t, filepath.Dir(client), "other", "wow", "12.1.0.69875")
			fake.windows = []desktop.WindowIdentity{testWindow(1, client), testWindow(2, other)}
			fake.respond[1] = func(command string) []*desktop.CapturedFrame {
				if command == "/dev connect" {
					return []*desktop.CapturedFrame{fake.frame(1, readyReceipt(func(signal *bridge.Signal) {
						signal.InputReady, signal.Sequence = true, 2
					}))}
				}
				return identityResponder(fake, 1, nil)(command)
			}
			alias := client
			switch spelling {
			case "slash":
				alias = filepath.ToSlash(client)
			case "trailing-separator":
				alias += string(os.PathSeparator)
			case "dot":
				alias += string(os.PathSeparator) + "."
			case "relative":
				t.Chdir(filepath.Dir(client))
				alias = filepath.Base(client)
			case "case":
				alias = strings.ToUpper(client)
			}
			connection, err := connectWindow(context.Background(), root, ConnectRequest{
				Snapshot: testPin(t, root), Installation: alias,
			}, fake.io())
			if err != nil || connection.Target.Window.Handle != 1 {
				t.Fatalf("alias %q selected %+v: %v", alias, connection, err)
			}
			reused, err := connectWindow(context.Background(), root, ConnectRequest{Session: connection.ID, Installation: alias}, fake.io())
			if err != nil || reused != connection {
				t.Fatalf("alias changed saved session: %+v %v", reused, err)
			}
			if _, err := connectWindow(context.Background(), root, ConnectRequest{Session: connection.ID, Installation: other}, fake.io()); !errors.Is(err, ErrSessionConstraint) {
				t.Fatalf("different installation accepted: %v", err)
			}
			for _, command := range fake.commands() {
				if strings.HasPrefix(command, "2:") {
					t.Fatalf("typed into excluded installation: %s", command)
				}
			}
		})
	}
}

func TestInstallationPathDoesNotEquateEmptyOrSiblingDirectories(t *testing.T) {
	path := t.TempDir()
	for _, other := range []string{"", path + "-other", filepath.Dir(path)} {
		if sameInstallationPath(path, other) {
			t.Fatalf("%q matched %q", path, other)
		}
	}
}
