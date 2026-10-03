package command

import (
	"context"
	"errors"
	"fmt"
	"testing"

	"github.com/follenfang/lycheedev/internal/live/duplex"
)

func TestChannelTraceFlushFailureHasPriority(t *testing.T) {
	writeFailure := errors.New("disk unavailable")
	flush := fmt.Errorf("%w: trace.json: %w", duplex.ErrPersistence, writeFailure)
	for _, original := range []error{nil, duplex.ErrPending, context.Canceled} {
		for _, joined := range []error{errors.Join(original, flush), errors.Join(flush, original)} {
			mapped := classifyChannelResultError(joined)
			exit, code, ok := channelFault(mapped)
			if !ok || exit != 5 || code != "live.duplex_persistence" {
				t.Fatalf("diagnostic failure masked: %d %s %v", exit, code, mapped)
			}
			if !errors.Is(mapped, duplex.ErrPersistence) || !errors.Is(mapped, writeFailure) || original != nil && !errors.Is(mapped, original) {
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
		{duplex.ErrPending, 6, "live.duplex_pending"},
		{context.Canceled, 7, "live.duplex_wait_cancelled"},
		{duplex.ErrRejected, 4, "live.duplex_rejected"},
		{errors.New("live.duplex_writer_profile_unverified"), 3, "live.duplex_writer_profile_unverified"},
	} {
		exit, code, ok := channelFault(classifyChannelResultError(tc.err))
		if !ok || exit != tc.exit || code != tc.code {
			t.Fatalf("changed normal mapping: %d %s", exit, code)
		}
	}
}
