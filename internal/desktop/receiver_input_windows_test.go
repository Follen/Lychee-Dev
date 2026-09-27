//go:build windows && amd64

package desktop

import (
	"context"
	"errors"
	"image"
	"strings"
	"testing"
	"time"
)

func TestReceiverInputRejectsWireBeforeMessage(t *testing.T) {
	f := openFixtureWindow(t, image.NewNRGBA(image.Rect(0, 0, 120, 80)))
	for _, frame := range []string{"/dev connect", "LDB1:bad\n", "LDB1:" + strings.Repeat("a", 181)} {
		receipt, err := WithReceiverInput(context.Background(), f.identity, func(context.Context) error { return nil }, func(in *ReceiverInput) error { return in.Stage(frame) })
		if err == nil || receipt.MessagesQueued != 0 {
			t.Fatalf("sent invalid frame %+v %v", receipt, err)
		}
	}
	select {
	case packet := <-f.packets:
		t.Fatalf("invalid frame sent packet %+v", packet)
	default:
	}
}

func TestReceiverCommitEndsWithOneEnterAndStopsBeforeItOnGuardFailure(t *testing.T) {
	for _, stopBeforeKey := range []bool{false, true} {
		t.Run(map[bool]string{false: "complete", true: "ownership_lost"}[stopBeforeKey], func(t *testing.T) {
			f := openFixtureWindow(t, image.NewNRGBA(image.Rect(0, 0, 120, 80)))
			const wire = "LDC1:test"
			checks := 0
			receipt, err := WithReceiverInput(context.Background(), f.identity, func(context.Context) error {
				checks++
				if stopBeforeKey && checks == len(wire)+2 {
					return errors.New("owner_changed")
				}
				return nil
			}, func(in *ReceiverInput) error { return in.Commit(wire) })
			want := len(wire) + 2
			if stopBeforeKey {
				want = len(wire)
			}
			if (err != nil) != stopBeforeKey || receipt.MessagesQueued != want || receipt.SubmissionComplete == stopBeforeKey {
				t.Fatalf("receipt=%+v err=%v", receipt, err)
			}
			timeout := time.After(time.Second)
			packets := want
			if !stopBeforeKey {
				packets++
			} // The fixture's TranslateMessage emits WM_CHAR for Enter.
			for index := 0; index < packets; index++ {
				select {
				case packet := <-f.packets:
					if index < len(wire) {
						if packet.message != 0x102 || packet.value != uintptr(wire[index]) {
							t.Fatalf("text packet %+v", packet)
						}
					} else if packet.message != []uint32{0x100, 0x102, 0x101}[index-len(wire)] || packet.value != 0x0D {
						t.Fatalf("final key packet %+v", packet)
					}
				case <-timeout:
					t.Fatal("native packet missing")
				}
			}
		})
	}
}

func TestFixedReloadRecordsBeforeEveryStep(t *testing.T) {
	f := openFixtureWindow(t, image.NewNRGBA(image.Rect(0, 0, 120, 80)))
	var steps []int
	receipt, err := WithReceiverInput(context.Background(), f.identity, func(context.Context) error { return nil }, func(in *ReceiverInput) error {
		return in.FixedReload(func(step int) error {
			steps = append(steps, step)
			if step == 3 {
				return errors.New("journal_failure")
			}
			return nil
		})
	})
	if err == nil || receipt.SubmissionComplete || receipt.MessagesQueued != 4 || len(steps) != 3 || steps[0] != 1 || steps[1] != 2 || steps[2] != 3 {
		t.Fatalf("progress=%v receipt=%+v err=%v", steps, receipt, err)
	}
}
