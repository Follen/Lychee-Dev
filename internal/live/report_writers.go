package live

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/desktop"
)

var ErrReportWriterAmbiguous = errors.New("live.report_writer_ambiguous")

// A second process sharing the installation is a possible SavedVariables
// writer even when no CLI owns it. Different character partitions are safe;
// an unknown peer or the same actor is refused before effects. This performs
// bounded WGC observation only: never input into another task's window.
func checkReportWriters(ctx context.Context, target ClientWindow, ready bridge.Signal, io *liveIO) error {
	windows, err := io.list(ctx)
	if err != nil {
		return err
	}
	seen := map[string]bool{}
	peers := 0
	for _, window := range windows {
		if !gameExecutable(window.Executable) || window.ProcessID == target.Window.ProcessID && window.ProcessStartedAt == target.Window.ProcessStartedAt {
			continue
		}
		identity := fmt.Sprintf("%d/%d", window.ProcessID, window.ProcessStartedAt)
		if seen[identity] {
			continue
		}
		seen[identity] = true
		client, err := windowClient(ctx, window, io)
		if err != nil {
			return errors.Join(ErrReportWriterAmbiguous, err)
		}
		if !sameInstallationPath(client.Directory, target.Client.Directory) {
			continue
		}
		peers++
		if peers > 8 || ready.ReportScope != "character-v1" {
			return ErrReportWriterAmbiguous
		}
		if err := observeDifferentWriter(ctx, ClientWindow{Client: client, Window: window}, ready, io); err != nil {
			return fmt.Errorf("%w: pid=%d: %v", ErrReportWriterAmbiguous, window.ProcessID, err)
		}
	}
	return nil
}

func observeDifferentWriter(ctx context.Context, target ClientWindow, ready bridge.Signal, io *liveIO) error {
	ctx, cancel := context.WithTimeout(ctx, 3*time.Second)
	defer cancel()
	if err := io.confirm(ctx, target); err != nil {
		return err
	}
	start := time.Now()
	frames, err := io.capture(ctx, target.Window, desktop.WholeWindowCapture())
	if err != nil {
		return err
	}
	defer frames.Close()
	// A single current frame is enough to identify a conflict. Two fresh
	// observations must agree before allowing a different partition.
	var prior bridge.Signal
	var ticks int64
	for count := 0; count < 24; count++ {
		frame, err := frames.Next(ctx)
		if err != nil {
			return err
		}
		if frame == nil || frame.NRGBA == nil || !frame.ObservedAt.After(start) || frame.SystemTicks <= ticks {
			continue
		}
		ticks = frame.SystemTicks
		symbols, err := desktop.DecodeSymbols(frame.NRGBA)
		if err != nil {
			continue
		}
		var observed bridge.Signal
		for _, symbol := range symbols {
			signals, err := bridge.ParseOpticalSignals(desktop.BytesFromSymbolText(symbol))
			if err != nil {
				continue
			}
			for _, signal := range signals {
				if signal.Kind != "ready" || signal.ReportScope != "character-v1" || signal.GUID == "" || signal.Character == "" || signal.Realm == "" || signal.Product != target.Client.Product || signal.Build != target.Client.FullBuild {
					continue
				}
				if observed.GUID != "" && observed.GUID != signal.GUID {
					return errors.New("conflicting_peer_identity")
				}
				observed = signal
			}
		}
		if observed.GUID == "" {
			continue
		}
		if strings.EqualFold(observed.Character, ready.Character) && strings.EqualFold(observed.Realm, ready.Realm) {
			return errors.New("same_character_writer")
		}
		if prior.GUID == observed.GUID && prior.Character == observed.Character && prior.Realm == observed.Realm && prior.SessionNonce == observed.SessionNonce && prior.RuntimeEpoch == observed.RuntimeEpoch && observed.Sequence >= prior.Sequence {
			return io.confirm(ctx, target)
		}
		prior = observed
	}
	return errors.New("peer_identity_not_fresh")
}

func (s *WindowSession) checkReportWriters(ctx context.Context) error {
	if s.writerIO == nil {
		return nil
	} // Injected sessions supply their own world.
	ready := s.ready
	ready.ReportScope = s.reportScope
	return checkReportWriters(ctx, s.target, ready, s.writerIO)
}
