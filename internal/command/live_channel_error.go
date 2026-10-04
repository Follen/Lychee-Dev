package command

import (
	"context"
	"errors"
	"strings"

	"github.com/follenfang/lycheedev/internal/live/duplex"
	"github.com/follenfang/lycheedev/internal/live/memory"
)

type channelCommandError struct {
	cause error
	code  string
	exit  int
}

func (e *channelCommandError) Error() string { return e.cause.Error() }
func (e *channelCommandError) Unwrap() error { return e.cause }

func classifyChannelResultError(err error) error {
	if err == nil {
		return nil
	}
	// Evidence persistence failure must remain visible even when the original
	// host wait was pending or cancelled. The business result is kept intact.
	if errors.Is(err, duplex.ErrPersistence) {
		return &channelCommandError{cause: err, code: "live.duplex_persistence", exit: 5}
	}
	if errors.Is(err, context.Canceled) {
		return &channelCommandError{cause: err, code: "live.duplex_wait_cancelled", exit: 7}
	}
	if mapped := classifyNativeLifecycleError(err); mapped != nil {
		return mapped
	}
	if errors.Is(err, duplex.ErrPending) || errors.Is(err, context.DeadlineExceeded) {
		return &channelCommandError{cause: err, code: "live.duplex_pending", exit: 6}
	}
	if errors.Is(err, duplex.ErrUnknown) {
		return &channelCommandError{cause: err, code: "live.duplex_execution_unknown", exit: 5}
	}
	if errors.Is(err, duplex.ErrBusy) || errors.Is(err, duplex.ErrBudget) {
		return &channelCommandError{cause: err, code: "live.duplex_pending", exit: 6}
	}
	if errors.Is(err, duplex.ErrIdentity) {
		return &channelCommandError{cause: err, code: "live.duplex_identity_changed", exit: 3}
	}
	if errors.Is(err, duplex.ErrRejected) {
		return &channelCommandError{cause: err, code: "live.duplex_rejected", exit: 4}
	}
	return classifyChannelError(err)
}

func classifyChannelError(err error) error {
	if mapped := classifyNativeLifecycleError(err); mapped != nil {
		return mapped
	}
	code := strings.SplitN(err.Error(), ":", 2)[0]
	if !strings.HasPrefix(code, "live.duplex_") {
		return err
	}
	exit := 4
	switch code {
	case "live.duplex_cancel_pending", "live.duplex_closing", "live.duplex_runtime_reloading", "live.duplex_character_not_in_world":
		exit = 6
	case "live.duplex_request_key_invalid", "live.duplex_request_conflict", "live.duplex_connection_id_required", "live.duplex_target_mismatch", "live.duplex_request_invalid":
		exit = 2
	case "live.duplex_writer_profile_unverified", "live.duplex_reload_state_unavailable", "live.duplex_world_state_unavailable", "live.duplex_activation_required", "live.duplex_clean_current_addon_required", "live.duplex_ownership_changed", "live.duplex_writer_layout_unsupported", "live.duplex_mailbox_required", "live.duplex_actor_mismatch", "live.duplex_legacy_retired":
		exit = 3
	}
	return &channelCommandError{cause: err, code: code, exit: exit}
}

func classifyNativeLifecycleError(err error) error {
	for _, item := range []struct {
		cause error
		exit  int
	}{
		{memory.ErrReloadUnknown, 3}, {memory.ErrWorldUnknown, 3},
		{memory.ErrReloadActive, 6}, {memory.ErrNotInWorld, 6},
	} {
		if errors.Is(err, item.cause) {
			return &channelCommandError{cause: err, code: item.cause.Error(), exit: item.exit}
		}
	}
	return nil
}

func channelFault(err error) (int, string, bool) {
	var e *channelCommandError
	if errors.As(err, &e) {
		return e.exit, e.code, true
	}
	return 0, "", false
}
