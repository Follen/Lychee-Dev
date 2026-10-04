package command

import (
	"context"
	"errors"
	"fmt"
	"testing"

	"github.com/follenfang/lycheedev/internal/live/duplex"
	"github.com/follenfang/lycheedev/internal/live/memory"
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

func TestChannelNativeLifecycleJoinedCauseKeepsCanonicalCode(t *testing.T) {
	for _, tc := range []struct {
		cause error
		exit  int
	}{
		{memory.ErrReloadUnknown, 3}, {memory.ErrWorldUnknown, 3},
		{memory.ErrReloadActive, 6}, {memory.ErrNotInWorld, 6},
	} {
		err := errors.Join(tc.cause, errors.New("short module read"))
		for _, mapped := range []error{classifyChannelError(err), classifyChannelResultError(err)} {
			exit, code, ok := channelFault(mapped)
			if !ok || exit != tc.exit || code != tc.cause.Error() || !errors.Is(mapped, tc.cause) {
				t.Fatalf("joined cause changed lifecycle code: %d %q %v", exit, code, mapped)
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
		{errors.New("live.duplex_runtime_reloading"), 6, "live.duplex_runtime_reloading"},
		{errors.New("live.duplex_character_not_in_world"), 6, "live.duplex_character_not_in_world"},
		{errors.New("live.duplex_world_state_unavailable"), 3, "live.duplex_world_state_unavailable"},
		{errors.New("live.duplex_reload_state_unavailable"), 3, "live.duplex_reload_state_unavailable"},
	} {
		exit, code, ok := channelFault(classifyChannelResultError(tc.err))
		if !ok || exit != tc.exit || code != tc.code {
			t.Fatalf("changed normal mapping: %d %s", exit, code)
		}
	}
}
