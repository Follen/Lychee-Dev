package command

import (
	"errors"
	"strings"
)

type channelCommandError struct {
	cause error
	code  string
	exit  int
}

func (e *channelCommandError) Error() string { return e.cause.Error() }
func (e *channelCommandError) Unwrap() error { return e.cause }

func classifyChannelError(err error) error {
	code := strings.SplitN(err.Error(), ":", 2)[0]
	if !strings.HasPrefix(code, "live.channel_") {
		return err
	}
	exit := 4
	switch code {
	case "live.channel_pending", "live.channel_discovery_incomplete", "live.channel_transaction_pending":
		exit = 6
	case "live.channel_request_key_invalid", "live.channel_request_invalid", "live.channel_request_conflict", "live.channel_connection_id_required", "live.channel_target_mismatch":
		exit = 2
	case "live.channel_execution_unknown", "live.channel_recovery_attempt_limit":
		exit = 5
	case "live.channel_activation_required", "live.channel_closed", "live.channel_not_idle", "live.channel_clean_current_addon_required", "live.channel_slot_installation_required", "live.channel_ownership_changed", "live.channel_reload_required", "live.channel_client_restart_required":
		exit = 3
	}
	return &channelCommandError{cause: err, code: code, exit: exit}
}

func channelFault(err error) (int, string, bool) {
	var e *channelCommandError
	if errors.As(err, &e) {
		return e.exit, e.code, true
	}
	return 0, "", false
}
