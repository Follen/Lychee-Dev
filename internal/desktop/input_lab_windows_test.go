//go:build windows && amd64 && lycheedev_input_lab

package desktop

import (
	"context"
	"errors"
	"image"
	"testing"
	"time"
	"unsafe"
)

func TestLabChordNativeMessagesAndCancellation(t *testing.T) {
	if unsafe.Sizeof(labNativeInput{}) != 40 {
		t.Fatal("invalid Win64 INPUT layout")
	}
	f := openFixtureWindow(t, image.NewNRGBA(image.Rect(0, 0, 120, 80)))
	receipt, err := WithLabInput(context.Background(), f.identity, "postmessage", func(context.Context) error { return nil }, func(in *LabInput) error { return in.Chord("wake") })
	if err != nil || receipt.MessagesQueued != 6 || !receipt.SubmissionComplete {
		t.Fatalf("%+v %v", receipt, err)
	}
	expected := []struct {
		message uint32
		vk      uintptr
	}{{0x100, 0x11}, {0x104, 0x12}, {0x104, 0xDD}, {0x105, 0xDD}, {0x105, 0x12}, {0x101, 0x11}}
	for _, e := range expected {
		select {
		case p := <-f.packets:
			if p.message != e.message || p.value != e.vk {
				t.Fatalf("%+v != %+v", p, e)
			}
		case <-time.After(time.Second):
			t.Fatal("missing key event")
		}
	}
	calls := 0
	_, err = WithLabInput(context.Background(), f.identity, "postmessage", func(context.Context) error {
		calls++
		if calls >= 3 {
			return errors.New("changed_owner")
		}
		return nil
	}, func(in *LabInput) error { return in.Chord("wake") })
	if err == nil {
		t.Fatal("lost owner was ignored")
	}
	// The pressed modifier must be released even after ownership changes.
	for _, vk := range []uintptr{0x11, 0x11} {
		select {
		case p := <-f.packets:
			if p.value != vk {
				t.Fatalf("unexpected key after failure: %+v", p)
			}
		case <-time.After(time.Second):
			t.Fatal("missing cancellation release")
		}
	}
}
func TestLabValidatesBeforeTyping(t *testing.T) {
	f := openFixtureWindow(t, image.NewNRGBA(image.Rect(0, 0, 120, 80)))
	for _, text := range []string{"", "line\nbreak", "中文"} {
		r, err := WithLabInput(context.Background(), f.identity, "postmessage", func(context.Context) error { return nil }, func(in *LabInput) error { return in.Text(text) })
		if err == nil || r.MessagesQueued != 0 {
			t.Fatalf("invalid text typed: %+v %v", r, err)
		}
	}
}
