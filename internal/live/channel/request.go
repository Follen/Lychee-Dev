package channel

import (
	"errors"
	"unicode/utf8"
)

func ValidateRequest(key string) error {
	if len(key) < 1 || len(key) > 256 || !utf8.ValidString(key) {
		return errors.New("live.channel_request_key_invalid")
	}
	for _, r := range key {
		if r < 32 || r == 127 {
			return errors.New("live.channel_request_key_invalid")
		}
	}
	return nil
}

// The journal is the request index. Looking up a completed request is read-only
// even when a later operation owns the connection; no process handle is needed.
func requestState(path, key string) (*State, error) {
	if err := ValidateRequest(key); err != nil {
		return nil, err
	}
	return historicalState(path, func(s State) bool { return s.Operation != nil && s.Operation.Request == key })
}

func checkRequest(s *State, code string, budget int, policy string) error {
	if s != nil && (s.Operation.Code != code || s.Operation.Budget != budget || s.Operation.Policy != policy) {
		return errors.New("live.channel_request_conflict")
	}
	return nil
}
