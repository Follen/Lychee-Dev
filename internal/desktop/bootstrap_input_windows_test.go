//go:build windows

package desktop

import (
	"context"
	"image"
	"strings"
	"testing"
)

func TestBootstrapInputSubmitsOnlyFixedStrings(t *testing.T) {
	f := openFixtureWindow(t, image.NewNRGBA(image.Rect(0, 0, 120, 80)))
	nonce := strings.Repeat("0123456789abcdef", 2)
	for _, command := range []string{"/dev bridge identify " + nonce, "/dev connect"} {
		receipt, err := QueueBootstrapCommand(context.Background(), f.identity, command)
		if err != nil || !receipt.SubmissionComplete || receipt.MessagesQueued < 3 {
			t.Fatalf("%q: %+v %v", command, receipt, err)
		}
	}
	for _, rejected := range []string{"/dev status", "/dev bridge ready", "/dev bridge load Request-A", "/dev connect extra", "garbage"} {
		receipt, err := QueueBootstrapCommand(context.Background(), f.identity, rejected)
		if err == nil || err.Error() != "desktop.bootstrap_command_not_allowed" || receipt.MessagesQueued != 0 {
			t.Fatalf("%q: %+v %v", rejected, receipt, err)
		}
	}
}

func TestBootstrapInputRespectsTheWindowInputLock(t *testing.T) {
	f := openFixtureWindow(t, image.NewNRGBA(image.Rect(0, 0, 120, 80)))
	started, release := make(chan struct{}), make(chan struct{})
	done := make(chan error, 1)
	go func() {
		_, err := withInputLock(context.Background(), f.identity, func() (InputReceipt, error) {
			close(started)
			<-release
			return InputReceipt{}, nil
		})
		done <- err
	}()
	<-started
	receipt, err := QueueBootstrapCommand(context.Background(), f.identity, "/dev connect")
	if err == nil || err.Error() != "desktop.input_busy" || receipt.MessagesQueued != 0 {
		t.Fatalf("busy bootstrap sent input: %+v %v", receipt, err)
	}
	close(release)
	if err := <-done; err != nil {
		t.Fatal(err)
	}
	receipt, err = QueueBootstrapCommand(context.Background(), f.identity, "/dev connect")
	if err != nil || !receipt.SubmissionComplete {
		t.Fatalf("released bootstrap: %+v %v", receipt, err)
	}
}
