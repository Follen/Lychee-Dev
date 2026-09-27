package live

import (
	"context"
	"crypto/sha256"
	"encoding/json"
	"errors"
	"fmt"
	"hash/adler32"
	"image"
	"image/color"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/live/journal"
)

type investigationFrames struct {
	*lifecycleFrames
	clear int
}

func (f *investigationFrames) Next(ctx context.Context) (*desktop.CapturedFrame, error) {
	if len(f.signals) > 0 || f.clear == 0 {
		return f.lifecycleFrames.Next(ctx)
	}
	f.clear--
	f.ticks++
	// Separate observations even on a Windows clock whose current tick is
	// shared with the just-archived checkpoint.
	time.Sleep(time.Millisecond)
	pixels := image.NewNRGBA(image.Rect(0, 0, 600, 600))
	for y := 0; y < 600; y++ {
		for x := 0; x < 600; x++ {
			pixels.SetNRGBA(x, y, color.NRGBA{80, 90, 100, 255})
		}
	}
	return &desktop.CapturedFrame{NRGBA: pixels, SystemTicks: f.ticks, ObservedAt: time.Now()}, nil
}

func TestCompleteInvestigationKeepsOwnerUntilDisplayProof(t *testing.T) {
	for _, mode := range []string{"complete", "interrupted-clear", "hidden-loaded", "hidden-report", "hidden-flush-ready", "hidden-ack-ready"} {
		t.Run(mode, func(t *testing.T) {
			interrupt := mode == "interrupted-clear"
			ctx := context.Background()
			root, client, book, pin, original, seed := unpreparedProbeFixture(t, 9)
			initial := original.ready
			initial.ReportScope = "character-v1"
			frames := &investigationFrames{lifecycleFrames: &lifecycleFrames{t: t, signals: []bridge.Signal{initial}}}
			expect := seed.Expected
			expect.Kind, expect.RequestID, expect.AfterSequence, expect.RequireInputReady = "ready", "", 0, true
			session, err := observeWindowSession(ctx, original.target, expect, frames, original.confirm)
			if err != nil {
				t.Fatal(err)
			}
			defer session.Close()
			record, err := session.prepareProbe(ctx, root, pin.ID, seed.Load.Account, "whole-task", "inline-revision", "finished", seed.Code, 10)
			if err != nil {
				t.Fatal(err)
			}
			input, definition, err := probeDefinition(record)
			if err != nil {
				t.Fatal(err)
			}
			if definition.Goal != "finished" || input.Load.ReportScope != "character-v1" {
				t.Fatal("full intent not frozen")
			}
			op, err := session.OpenOperation(ctx, root, record.OperationID)
			if err != nil {
				t.Fatal(err)
			}
			defer op.Close()
			body := `{"probeStatus":"completed","acceptedBudgetSeconds":10,"result":{"answer":42}}`
			base := func(kind string, seq, epoch uint64) bridge.Signal {
				s := initial
				s.Kind, s.Sequence, s.RuntimeEpoch, s.InputReady = kind, seq, epoch, false
				s.RequestID = definition.RequestID
				s.ReportScope = ""
				return s
			}
			ready := func(seq, epoch uint64, reentry bool) bridge.Signal {
				s := base("ready", seq, epoch)
				s.InputReady = true
				s.ReportScope = "character-v1"
				if reentry {
					s.ReloadNonce = definition.ReloadNonce
				} else {
					s.RequestID = ""
				}
				return s
			}
			loaded := base("loaded", 10, 0)
			loaded.InputReady = true
			loaded.ReloadNonce = definition.ReloadNonce
			loaded.CodeBytes, loaded.CodeAdler32 = uint32(len(seed.Code)), fmt.Sprintf("%08x", adler32.Checksum(seed.Code))
			reported := loaded
			reported.Kind, reported.Sequence, reported.ReloadNonce, reported.InputReady = "reported", 11, "", false
			reported.ReportBytes, reported.ReportAdler32 = uint32(len(body)), fmt.Sprintf("%08x", adler32.Checksum([]byte(body)))
			ack := reported
			ack.Kind, ack.Sequence = "acknowledged", 50
			commands := []string{}
			finishes := 0
			observations := 0
			send := func(ctx context.Context, target desktop.WindowIdentity, prepare func(context.Context) (string, error), guard func(context.Context) error) (desktop.InputReceipt, error) {
				if target != session.target.Window {
					t.Fatal("retargeted")
				}
				current, err := book.InspectWork(ctx, record.OperationID)
				if err != nil {
					t.Fatal(err)
				}
				businessCount := 0
				for _, sent := range commands {
					if !strings.Contains(sent, "bridge observe ") && !strings.Contains(sent, "bridge finish ") {
						businessCount++
					}
				}
				switch businessCount {
				case 0:
					frames.signals = append(frames.signals, initial)
				case 1:
					frames.signals = append(frames.signals, ready(1, 2, true))
				case 2:
					if mode != "hidden-loaded" || observations > 0 {
						frames.signals = append(frames.signals, loaded)
					}
				case 3:
					if mode != "hidden-flush-ready" {
						frames.signals = append(frames.signals, ready(12, 2, false))
					}
				case 4:
					if mode != "hidden-ack-ready" {
						frames.signals = append(frames.signals, ready(1, 3, true))
					}
				}
				_ = current
				if err := guard(ctx); err != nil {
					return desktop.InputReceipt{}, err
				}
				command, err := prepare(ctx)
				if err != nil {
					return desktop.InputReceipt{}, err
				}
				if err := guard(ctx); err != nil {
					return desktop.InputReceipt{}, err
				}
				commands = append(commands, command)
				parts := strings.Fields(command)
				switch parts[2] {
				case "prepare":
					frames.signals = append(frames.signals, ready(1, 2, true))
				case "load":
					if mode != "hidden-loaded" {
						frames.signals = append(frames.signals, loaded)
					}
				case "run":
					if mode != "hidden-report" {
						frames.signals = append(frames.signals, reported)
					}
				case "reload":
					raw, _ := json.Marshal(reported)
					path := filepath.Join(client, "WTF", "Account", input.Load.Account, input.Expected.Realm, input.Expected.Character, "SavedVariables", "Lychee Dev.lua")
					if err := os.MkdirAll(filepath.Dir(path), 0700); err != nil {
						t.Fatal(err)
					}
					if err := os.WriteFile(path, []byte(fmt.Sprintf(`LycheeToolkitBridgeDB={schema=1,reports={[%q]={receipt=%q,body=%q}}}`, definition.RequestID, raw, body)), 0600); err != nil {
						t.Fatal(err)
					}
					if mode != "hidden-ack-ready" {
						frames.signals = append(frames.signals, ready(1, 3, true))
					}
				case "ack":
					frames.signals = append(frames.signals, ack)
				case "observe":
					observations++
					epoch := uint64(2)
					if businessCount >= 4 {
						epoch = 3
					}
					s := base("checkpoint", 30+uint64(observations), epoch)
					s.WorkState = "reported"
					s.ProbeNonce = parts[4]
					s.CodeSHA256 = fmt.Sprintf("%x", sha256.Sum256(seed.Code))
					s.ResourcesReleased = true
					raw, _ := json.Marshal(reported)
					s.Receipt = string(raw)
					if businessCount == 2 {
						s.WorkState, s.Receipt, s.Sequence = "loaded", "", loaded.Sequence-1
						frames.signals = append(frames.signals, s, loaded)
						break
					}
					frames.signals = append(frames.signals, s, ready(s.Sequence+1, epoch, false))
				case "finish":
					finishes++
					owner, busy, err := journal.InspectWindowOwner(ctx, filepath.Join(client, "Interface", "AddOns"), record.Intent.Resource)
					if err != nil || !busy || owner.OperationID != record.OperationID {
						t.Fatal("window released before display proof", err)
					}
					if len(operationQueue(t, client)) != 0 {
						t.Fatal("queue was not retired")
					}
					s := base("checkpoint", uint64(60+finishes), 3)
					s.WorkState = "finished"
					s.ProbeNonce = parts[4]
					s.CodeSHA256 = fmt.Sprintf("%x", sha256.Sum256(seed.Code))
					s.ResourcesReleased = true
					raw, _ := json.Marshal(ack)
					s.Receipt = string(raw)
					frames.signals = append(frames.signals, s)
					if !interrupt || finishes > 1 {
						frames.clear = 2
					}
				default:
					t.Fatal("unexpected input", command)
				}
				return desktop.InputReceipt{SubmissionComplete: true, MessagesQueued: 1}, nil
			}
			record, err = op.execute(ctx, send)
			if interrupt {
				if !errors.Is(err, ErrReceiptHidePending) || record.Stage != "acknowledged" {
					t.Fatalf("%+v %v", record, err)
				}
				status, statusErr := Status(ctx, root, record.OperationID)
				if statusErr != nil || status.Complete || status.Report.State != "verified" || status.Cleanup != "pending" || status.NextAction == nil || status.NextAction.Args[2] != record.OperationID {
					t.Fatalf("%+v %v", status, statusErr)
				}
				record, err = op.execute(ctx, send)
			}
			if err != nil || record.Stage != "cleaned" {
				t.Fatalf("%+v %v\n%v", record, err, commands)
			}
			for i := 0; i < 2; i++ {
				result, err := Resume(ctx, root, record.OperationID)
				if err != nil || !result.Complete || result.Display == nil || result.Display.State != "cleared" || result.NextAction != nil {
					t.Fatalf("%+v %v", result, err)
				}
			}
			runs, acks := 0, 0
			for _, command := range commands {
				if strings.Contains(command, "bridge run ") {
					runs++
				}
				if strings.Contains(command, "bridge ack ") {
					acks++
				}
			}
			if runs != 1 || acks != 1 || finishes != 1+map[bool]int{true: 1}[interrupt] {
				t.Fatal("replayed business", commands)
			}
			if strings.HasPrefix(mode, "hidden-") && observations == 0 {
				t.Fatal("missing card recovered without correlated observation")
			}
		})
	}
}
