//go:build windows && amd64

package channel

import (
	"context"
	"testing"
)

func TestRetiredOpticalInputCannotSend(t *testing.T) {
	n := &Native{Guard: func(context.Context) error { t.Fatal("retired input reached native guard"); return nil }}
	out, err := n.Input(context.Background(), InputAction{Kind: "invoke", Capability: "lycheedev.input.hybrid.v2"})
	if err == nil || out.Disposition != "not_sent" || out.MessagesQueued != 0 || out.Reason != "input_capability_unsupported" || n.Publication != nil {
		t.Fatal(out, err)
	}
}
