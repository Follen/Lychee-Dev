package live

import (
	"context"
	"errors"
	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/selection"
	"image"
	"testing"
)

func TestReportWritersDistinguishProcessesAndCharacterPartitions(t *testing.T) {
	for _, mode := range []string{"single", "same-process", "other-install", "different-character", "same-character", "unobserved", "account-scope", "peer-changed"} {
		t.Run(mode, func(t *testing.T) {
			client, fake, _ := connectFixture(t)
			installation, err := fake.io().inspect(context.Background(), client)
			if err != nil {
				t.Fatal(err)
			}
			target := ClientWindow{Client: installation, Window: testWindow(1, client)}
			ready := readyReceipt(nil)
			ready.ReportScope = "character-v1"
			peer := testWindow(2, client)
			windows := []desktop.WindowIdentity{target.Window, peer}
			if mode == "single" {
				windows = windows[:1]
			}
			if mode == "same-process" {
				windows[1] = target.Window
				windows[1].Handle++
			}
			if mode == "account-scope" {
				ready.ReportScope = ""
			}
			seen := 0
			io := &liveIO{
				list: func(context.Context) ([]desktop.WindowIdentity, error) { return windows, nil },
				inspect: func(context.Context, string) (selection.ClientInstallation, error) {
					other := installation
					if mode == "other-install" {
						other.Directory += "-second"
					}
					return other, nil
				},
				confirm: func(context.Context, ClientWindow) error {
					if mode == "peer-changed" {
						return ErrActorChanged
					}
					return nil
				},
				capture: func(context.Context, desktop.WindowIdentity, image.Rectangle) (sessionFrames, error) {
					seen++
					a := readyReceipt(nil)
					a.ReportScope = "character-v1"
					if mode != "same-character" {
						a.Character += "Other"
						a.GUID += "Other"
					}
					frames := &lifecycleFrames{t: t, signals: []bridge.Signal{a, a}}
					if mode == "unobserved" {
						frames.signals = nil
					}
					return frames, nil
				},
			}
			err = checkReportWriters(context.Background(), target, ready, io)
			blocked := mode == "same-character" || mode == "unobserved" || mode == "account-scope" || mode == "peer-changed"
			if blocked != errors.Is(err, ErrReportWriterAmbiguous) || !blocked && err != nil {
				t.Fatalf("%s: %v", mode, err)
			}
			if (mode == "single" || mode == "same-process" || mode == "other-install") && seen != 0 {
				t.Fatal("captured unrelated process")
			}
		})
	}
}
