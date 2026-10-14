//go:build windows && amd64

package desktop

import (
	"context"
	"errors"
	"image"
	"testing"
	"time"
)

func TestMailboxReloadUsesDistinctFunctionKeyWithoutChat(t *testing.T) {
	f := openFixtureWindow(t, image.NewNRGBA(image.Rect(0, 0, 120, 80)))
	b := MailboxBindings{Wake: "ALT-CTRL-F12", Reload: "ALT-CTRL-F11"}
	receipt, err := WithMailboxInput(context.Background(), f.identity, b, func(context.Context) error { return nil }, func(_, reload func() error) error { return reload() })
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
	receipt, err := WithMailboxInput(ctx, f.identity, MailboxBindings{Wake: "ALT-CTRL-F12", Reload: "ALT-CTRL-F11"}, guard, func(wake, reload func() error) error { return wake() })
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
