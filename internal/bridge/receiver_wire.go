package bridge

import (
	"crypto/rand"
	"encoding/hex"
	"errors"
	"fmt"
	"hash/adler32"
	"strconv"
	"strings"
)

const MaxReceiverWireBytes = 256
const MaxHostReceiverStageBytes = 180

type ReceiverStage struct {
	ReceiverNonce string
	RuntimeEpoch  uint64
	RequestID     string
	AttemptID     string
	Action        string
	Arg           string
}

type ReceiverCommit struct {
	ReceiverNonce string
	AttemptID     string
	CommitNonce   string
	BodyAdler32   string
}

func NewReceiverAttemptID() (string, error) {
	var bytes [8]byte
	if _, err := rand.Read(bytes[:]); err != nil {
		return "", err
	}
	return hex.EncodeToString(bytes[:]), nil
}

func receiverDecimal(s string, maxDigits int) bool {
	if len(s) == 0 || len(s) > maxDigits || len(s) > 1 && s[0] == '0' {
		return false
	}
	for _, c := range s {
		if c < '0' || c > '9' {
			return false
		}
	}
	return s != "0"
}

func receiverKey(s string, max int) bool {
	if len(s) == 0 || len(s) > max {
		return false
	}
	for _, c := range s {
		if !(c >= 'A' && c <= 'Z' || c >= 'a' && c <= 'z' || c >= '0' && c <= '9' || c == '_' || c == '-') {
			return false
		}
	}
	return true
}

func validReceiverAction(action, request, arg string) bool {
	switch action {
	case "identify", "reset", "refresh":
		return request == "-" && queueHex(arg, 32)
	case "connect", "ready", "hide":
		return request == "-" && arg == "-"
	case "load", "run", "reload":
		return request != "-" && arg == "-"
	case "verify", "prepare", "clean", "flush", "observe", "finish":
		return request != "-" && queueHex(arg, 32)
	case "ack", "bugs-ack":
		return request != "-" && receiverDecimal(arg, 16)
	case "bugs":
		if request == "-" || len(arg) == 0 || len(arg) > 3 {
			return false
		}
		for _, c := range arg {
			if c < '0' || c > '9' {
				return false
			}
		}
		return true
	default:
		return false
	}
}

// ReceiverBody is the frozen business content, stable across retry attempts.
func (s ReceiverStage) ReceiverBody() (string, error) {
	if !receiverKey(s.RequestID, 80) || !receiverKey(s.Action, 16) || !validReceiverAction(s.Action, s.RequestID, s.Arg) {
		return "", errors.New("bridge.receiver_action_invalid")
	}
	return s.Action + "|" + s.RequestID + "|" + s.Arg, nil
}

func (s ReceiverStage) BodyAdler32() (string, error) {
	body, err := s.ReceiverBody()
	if err != nil {
		return "", err
	}
	return fmt.Sprintf("%08x", adler32.Checksum([]byte(body))), nil
}

func EncodeReceiverStage(s ReceiverStage) (string, error) {
	body, err := s.ReceiverBody()
	if err != nil || !queueHex(s.ReceiverNonce, 32) || !queueHex(s.AttemptID, 16) || s.RuntimeEpoch == 0 || s.RuntimeEpoch > 9007199254740990 {
		return "", errors.New("bridge.receiver_stage_invalid")
	}
	prefix := fmt.Sprintf("LDB1:%s:%d:%s:%s:%s:%s:%d", s.ReceiverNonce, s.RuntimeEpoch, s.RequestID, s.AttemptID, s.Action, s.Arg, len(body))
	wire := fmt.Sprintf("%s:%08x", prefix, adler32.Checksum([]byte(prefix)))
	if len(wire) > MaxHostReceiverStageBytes {
		return "", errors.New("bridge.receiver_host_stage_capacity")
	}
	return wire, nil
}

func EncodeReceiverCommit(c ReceiverCommit) (string, error) {
	if !queueHex(c.ReceiverNonce, 32) || !queueHex(c.AttemptID, 16) || !queueHex(c.CommitNonce, 16) || !queueHex(c.BodyAdler32, 8) {
		return "", errors.New("bridge.receiver_commit_invalid")
	}
	prefix := fmt.Sprintf("LDC1:%s:%s:%s:%s", c.ReceiverNonce, c.AttemptID, c.CommitNonce, c.BodyAdler32)
	return fmt.Sprintf("%s:%08x", prefix, adler32.Checksum([]byte(prefix))), nil
}

// ParseReceiverStage and ParseReceiverCommit are strict mirrors of the Lua
// grammar, useful for local validation and cross-language protocol fixtures.
func ParseReceiverStage(wire string) (ReceiverStage, error) {
	var s ReceiverStage
	if len(wire) > MaxReceiverWireBytes {
		return s, errors.New("bridge.receiver_wire_capacity")
	}
	parts := strings.Split(wire, ":")
	if len(parts) != 9 || parts[0] != "LDB1" {
		return s, errors.New("bridge.receiver_stage_invalid")
	}
	size, err := strconv.Atoi(parts[7])
	if err != nil || size <= 0 || !receiverDecimal(parts[2], 16) || !receiverDecimal(parts[7], 3) || !queueHex(parts[8], 8) {
		return s, errors.New("bridge.receiver_stage_invalid")
	}
	epoch, err := strconv.ParseUint(parts[2], 10, 64)
	if err != nil {
		return s, errors.New("bridge.receiver_stage_invalid")
	}
	s = ReceiverStage{ReceiverNonce: parts[1], RuntimeEpoch: epoch, RequestID: parts[3], AttemptID: parts[4], Action: parts[5], Arg: parts[6]}
	body, err := s.ReceiverBody()
	if err != nil || len(body) != size || !queueHex(s.ReceiverNonce, 32) || !queueHex(s.AttemptID, 16) || epoch > 9007199254740990 || fmt.Sprintf("%08x", adler32.Checksum([]byte(strings.Join(parts[:8], ":")))) != parts[8] {
		return ReceiverStage{}, errors.New("bridge.receiver_stage_invalid")
	}
	return s, nil
}

func ParseReceiverCommit(wire string) (ReceiverCommit, error) {
	var c ReceiverCommit
	if len(wire) > MaxReceiverWireBytes {
		return c, errors.New("bridge.receiver_wire_capacity")
	}
	parts := strings.Split(wire, ":")
	if len(parts) != 6 || parts[0] != "LDC1" || !queueHex(parts[5], 8) || fmt.Sprintf("%08x", adler32.Checksum([]byte(strings.Join(parts[:5], ":")))) != parts[5] {
		return c, errors.New("bridge.receiver_commit_invalid")
	}
	c = ReceiverCommit{ReceiverNonce: parts[1], AttemptID: parts[2], CommitNonce: parts[3], BodyAdler32: parts[4]}
	if !queueHex(c.ReceiverNonce, 32) || !queueHex(c.AttemptID, 16) || !queueHex(c.CommitNonce, 16) || !queueHex(c.BodyAdler32, 8) {
		return ReceiverCommit{}, errors.New("bridge.receiver_commit_invalid")
	}
	return c, nil
}

// MatchReceiverReadback checks the immutable stage identity before the host
// may answer a challenge. A nonce alone cannot authorize a business action.
func MatchReceiverReadback(stage ReceiverStage, signal Signal, kind string) error {
	body, err := stage.ReceiverBody()
	if err != nil {
		return err
	}
	digest, err := stage.BodyAdler32()
	if err != nil {
		return err
	}
	if signal.Kind != kind || signal.ReceiverNonce != stage.ReceiverNonce || signal.RuntimeEpoch != stage.RuntimeEpoch || signal.RequestID != stage.RequestID || signal.AttemptID != stage.AttemptID || signal.BodyBytes != uint32(len(body)) || signal.BodyAdler32 != digest {
		return errors.New("bridge.receiver_readback_mismatch")
	}
	switch kind {
	case "receiver_staged":
		if !signal.InputReady || signal.Accepted || signal.CommitNonce != "" {
			return errors.New("bridge.receiver_readback_mismatch")
		}
	case "receiver_commit_ready":
		if !signal.InputReady || signal.Accepted || !queueHex(signal.CommitNonce, 16) {
			return errors.New("bridge.receiver_readback_mismatch")
		}
	case "receiver_accepted":
		if signal.InputReady || !signal.Accepted {
			return errors.New("bridge.receiver_readback_mismatch")
		}
	default:
		return errors.New("bridge.receiver_readback_kind")
	}
	return nil
}

func CommitForReceiverChallenge(stage ReceiverStage, challenge Signal) (ReceiverCommit, error) {
	if err := MatchReceiverReadback(stage, challenge, "receiver_commit_ready"); err != nil {
		return ReceiverCommit{}, err
	}
	digest, _ := stage.BodyAdler32()
	return ReceiverCommit{ReceiverNonce: stage.ReceiverNonce, AttemptID: stage.AttemptID, CommitNonce: challenge.CommitNonce, BodyAdler32: digest}, nil
}
