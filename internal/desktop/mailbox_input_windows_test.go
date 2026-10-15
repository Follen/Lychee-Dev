//go:build windows && amd64

package desktop

import (
	"context"
	"errors"
	"image"
	"testing"
	"time"
)

func fixtureKeyboardReady() error { return nil }

func TestMailboxReloadUsesDistinctFunctionKeyWithoutChat(t *testing.T) {
	f := openFixtureWindow(t, image.NewNRGBA(image.Rect(0, 0, 120, 80)))
	b := MailboxBindings{Wake: "ALT-CTRL-F12", Reload: "ALT-CTRL-F11"}
	receipt, err := WithMailboxInput(context.Background(), f.identity, b, func(context.Context) error { return nil }, fixtureKeyboardReady, func(_, reload func() error) error { return reload() })
	if err != nil || !receipt.SubmissionComplete || receipt.MessagesQueued != 6 {
		t.Fatalf("receipt=%+v error=%v", receipt, err)
	}
	keys := []uintptr{0x11, 0x12, 0x7A, 0x7A, 0x12, 0x11}
	messages := []uint32{0x100, 0x104, 0x104, 0x105, 0x105, 0x101}
	for i, key := range keys {
		select {
		case p := <-f.packets:
			if p.value != key || p.message != messages[i] || p.bits&0xffff != 1 || (i >= 3 && p.bits&(1<<30|1<<31) != (1<<30|1<<31)) || (i > 0 && i < 5 && p.bits&(1<<29) == 0) {
				t.Fatalf("packet %d = %+v", i, p)
			}
		case <-time.After(time.Second):
			t.Fatal("missing key message")
		}
	}
}

func TestMailboxCancelledChordReleasesOnlyInjectedKeys(t *testing.T) {
	f := openFixtureWindow(t, image.NewNRGBA(image.Rect(0, 0, 120, 80)))
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	checks := 0
	guard := func(context.Context) error {
		checks++
		if checks == 3 {
			cancel()
		}
		return ctx.Err()
	}
	receipt, err := WithMailboxInput(ctx, f.identity, MailboxBindings{Wake: "ALT-CTRL-F12", Reload: "ALT-CTRL-F11"}, guard, fixtureKeyboardReady, func(wake, reload func() error) error { return wake() })
	if !errors.Is(err, context.Canceled) || receipt.SubmissionComplete || receipt.MessagesQueued != 2 {
		t.Fatalf("receipt=%+v err=%v", receipt, err)
	}
	for _, want := range []uint32{0x100, 0x101} {
		select {
		case p := <-f.packets:
			if p.message != want || p.value != 0x11 {
				t.Fatal("released unrelated key or retained modifier", p)
			}
		case <-time.After(time.Second):
			t.Fatal("missing release")
		}
	}
}

func TestMailboxKeyboardActivityRequiresFreshInputBeforeRetry(t *testing.T) {
	for _, duringRead := range []bool{false, true} {
		name := "before_input_read"
		if duringRead {
			name = "during_input_read"
		}
		t.Run(name, func(t *testing.T) {
			f := openFixtureWindow(t, image.NewNRGBA(image.Rect(0, 0, 120, 80)))
			now := time.Now()
			state := &keyboardActivityState{lastEvent: now.Add(-time.Second)}
			ready := func() error { return state.check(now, func(uint16) bool { return false }) }
			if !duringRead {
				state.note(now)
			}
			reads := 0
			send := func(wake, reload func() error) error {
				reads++
				if duringRead && reads == 1 {
					state.note(now)
				}
				return wake()
			}
			b := MailboxBindings{Wake: "ALT-CTRL-F12", Reload: "ALT-CTRL-F11"}
			receipt, err := WithMailboxInput(context.Background(), f.identity, b, func(context.Context) error { return nil }, ready, send)
			if !errors.Is(err, ErrKeyboardActive) || receipt.MessagesQueued != 0 || receipt.SubmissionComplete {
				t.Fatalf("busy keyboard sent input: %+v %v", receipt, err)
			}
			wantReads := 0
			if duringRead {
				wantReads = 1
			}
			if reads != wantReads {
				t.Fatalf("busy input reads=%d want=%d", reads, wantReads)
			}
			select {
			case packet := <-f.packets:
				t.Fatalf("busy generated packet: %+v", packet)
			default:
			}
			now = now.Add(500 * time.Millisecond)
			receipt, err = WithMailboxInput(context.Background(), f.identity, b, func(context.Context) error { return nil }, ready, send)
			if err != nil || receipt.MessagesQueued != 6 || !receipt.SubmissionComplete || reads != wantReads+1 {
				t.Fatalf("quiet retry did not reread INPUT and submit once: %+v reads=%d err=%v", receipt, reads, err)
			}
		})
	}
}

func TestMailboxActivityDuringChordFinishesAndReleasesModifiers(t *testing.T) {
	f := openFixtureWindow(t, image.NewNRGBA(image.Rect(0, 0, 120, 80)))
	busy := false
	checks := 0
	guard := func(context.Context) error {
		checks++
		if checks == 3 { // Ctrl-down has already been queued.
			busy = true
		}
		return nil
	}
	ready := func() error {
		if busy {
			return ErrKeyboardActive
		}
		return nil
	}
	b := MailboxBindings{Wake: "ALT-CTRL-F12", Reload: "ALT-CTRL-F11"}
	receipt, err := WithMailboxInput(context.Background(), f.identity, b, guard, ready, func(wake, reload func() error) error { return wake() })
	if err != nil || !busy || !receipt.SubmissionComplete || receipt.MessagesQueued != 6 {
		t.Fatalf("activity interrupted an active chord: %+v %v", receipt, err)
	}
	for i, key := range []uintptr{0x11, 0x12, 0x7B, 0x7B, 0x12, 0x11} {
		select {
		case p := <-f.packets:
			if p.value != key || (i >= 3 && p.message != 0x101 && p.message != 0x105) {
				t.Fatalf("modifier release %d: %+v", i, p)
			}
		case <-time.After(time.Second):
			t.Fatal("missing chord/release packet")
		}
	}
	receipt, err = WithMailboxInput(context.Background(), f.identity, b, guard, ready, func(wake, reload func() error) error { return wake() })
	if !errors.Is(err, ErrKeyboardActive) || receipt.MessagesQueued != 0 {
		t.Fatalf("next chord ignored keyboard activity: %+v %v", receipt, err)
	}
}
