package command

import (
	"context"
	"errors"
	"fmt"
	"testing"

	"github.com/follenfang/lycheedev/internal/live/channel"
)

func TestChannelTraceFlushFailureHasPriority(t *testing.T) {
	writeFailure := errors.New("disk unavailable")
	flush := fmt.Errorf("%w: trace.json: %w", channel.ErrTraceFlush, writeFailure)
	for _, original := range []error{nil, channel.ErrPending, context.Canceled} {
		for _, joined := range []error{errors.Join(original, flush), errors.Join(flush, original)} {
			mapped := classifyChannelResultError(joined)
			exit, code, ok := channelFault(mapped)
			if !ok || exit != 5 || code != "live.channel_trace_flush" {
				t.Fatalf("diagnostic failure masked: %d %s %v", exit, code, mapped)
			}
			if !errors.Is(mapped, channel.ErrTraceFlush) || !errors.Is(mapped, writeFailure) || original != nil && !errors.Is(mapped, original) {
				t.Fatalf("lost cause: %v", mapped)
			}
		}
	}
}

func TestChannelResultWithoutFlushFailureKeepsClassification(t *testing.T) {
	if classifyChannelResultError(nil) != nil {
		t.Fatal("successful command gained an error")
	}
	for _, tc := range []struct {
		err  error
		exit int
		code string
	}{
		{channel.ErrPending, 6, "live.channel_pending"},
		{context.Canceled, 7, "live.channel_wait_cancelled"},
		{errors.New("live.channel_execution_unknown"), 5, "live.channel_execution_unknown"},
		{errors.New("live.channel_input_capability_unsupported"), 3, "live.channel_input_capability_unsupported"},
		{errors.New("live.channel_input_capability_unsupported: future capability"), 3, "live.channel_input_capability_unsupported"},
	} {
		exit, code, ok := channelFault(classifyChannelResultError(tc.err))
		if !ok || exit != tc.exit || code != tc.code {
			t.Fatalf("changed normal mapping: %d %s", exit, code)
		}
	}
}
