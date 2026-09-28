package channel

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"github.com/follenfang/lycheedev/internal/vault"
	"os"
	"path/filepath"
)

func artifactPath(log, digest string) string {
	return filepath.Join(filepath.Dir(log), "artifacts", digest)
}
func readArtifact(log, digest string) ([]byte, error) {
	raw, err := hex.DecodeString(digest)
	if err != nil || len(raw) != 32 || hex.EncodeToString(raw) != digest {
		return nil, errors.New("live.channel_artifact_invalid")
	}
	path := artifactPath(log, digest)
	stat, err := os.Stat(path)
	if err != nil {
		return nil, err
	}
	if !stat.Mode().IsRegular() || stat.Size() > 524288 {
		return nil, errors.New("live.channel_artifact_invalid")
	}
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, err
	}
	sum := sha256.Sum256(data)
	if hex.EncodeToString(sum[:]) != digest {
		return nil, errors.New("live.channel_artifact_corrupt")
	}
	return data, nil
}
func snapshot(ctx context.Context, log string, s State) (State, error) {
	data, err := json.Marshal(s)
	if err != nil {
		return State{}, err
	}
	var copy State
	if err = json.Unmarshal(data, &copy); err != nil {
		return State{}, err
	}
	copy.Artifacts = map[string]string{}
	put := func(key string, data []byte) error {
		if len(data) == 0 {
			return nil
		}
		sum := sha256.Sum256(data)
		digest := hex.EncodeToString(sum[:])
		if _, err := readArtifact(log, digest); errors.Is(err, os.ErrNotExist) {
			if err = vault.ReplaceFile(ctx, artifactPath(log, digest), data); err != nil {
				return err
			}
		} else if err != nil {
			return err
		}
		copy.Artifacts[key] = digest
		return nil
	}
	if op := copy.Operation; op != nil {
		if err = put("code", []byte(op.Code)); err != nil {
			return State{}, err
		}
		op.Code = ""
		if err = put("result", op.Result); err != nil {
			return State{}, err
		}
		op.Result = nil
	}
	if tx := copy.Transaction; tx != nil {
		if err = put("transaction", []byte(tx.Envelope.Code)); err != nil {
			return State{}, err
		}
		tx.Envelope.Code = ""
	}
	if r := copy.Recovery; r != nil && r.Transaction != nil {
		if err = put("recovery", []byte(r.Transaction.Envelope.Code)); err != nil {
			return State{}, err
		}
		r.Transaction.Envelope.Code = ""
	}
	return copy, nil
}
func hydrate(log string, s *State) error {
	for key, digest := range s.Artifacts {
		data, err := readArtifact(log, digest)
		if err != nil {
			return err
		}
		switch key {
		case "code":
			if s.Operation == nil {
				return errors.New("live.channel_artifact_invalid")
			}
			s.Operation.Code = string(data)
		case "result":
			if s.Operation == nil {
				return errors.New("live.channel_artifact_invalid")
			}
			s.Operation.Result = data
		case "transaction":
			if s.Transaction == nil {
				return errors.New("live.channel_artifact_invalid")
			}
			s.Transaction.Envelope.Code = string(data)
		case "recovery":
			if s.Recovery == nil || s.Recovery.Transaction == nil {
				return errors.New("live.channel_artifact_invalid")
			}
			s.Recovery.Transaction.Envelope.Code = string(data)
		default:
			return errors.New("live.channel_artifact_invalid")
		}
	}
	s.Artifacts = nil
	return s.validate()
}
