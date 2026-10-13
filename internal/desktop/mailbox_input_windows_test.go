//go:build windows && amd64

package desktop

import (
	"context"
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
			if p.value != key || p.message != messages[i] {
				t.Fatalf("packet %d = %+v", i, p)
			}
		case <-time.After(time.Second):
			t.Fatal("missing key message")
		}
	}
}
