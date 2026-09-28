package journal

import (
	"bufio"
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"

	"github.com/follenfang/lycheedev/internal/vault"
)

var ErrMemoryTornTail = errors.New("journal.memory_torn_tail")

type MemoryEvent struct {
	Sequence int             `json:"sequence"`
	Previous string          `json:"previous"`
	Kind     string          `json:"kind"`
	Data     json.RawMessage `json:"data"`
	Checksum string          `json:"checksum"`
}

func memoryEventHash(event MemoryEvent) string {
	event.Checksum = ""
	b, _ := json.Marshal(event)
	return fmt.Sprintf("%x", sha256.Sum256(b))
}

// ReadMemoryLog validates the whole append-only chain. A torn tail is explicit;
// callers cannot silently turn a damaged operation into a new empty one.
func ReadMemoryLog(path string) ([]MemoryEvent, error) {
	if info, err := os.Lstat(path); err == nil && !info.Mode().IsRegular() {
		return nil, errors.New("journal.memory_log_not_regular")
	}
	f, err := os.Open(path)
	if errors.Is(err, os.ErrNotExist) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	defer f.Close()
	info, err := f.Stat()
	if err != nil {
		return nil, err
	}
	if !info.Mode().IsRegular() || info.Size() > 64<<20 {
		return nil, errors.New("journal.memory_log_limit")
	}
	reader := bufio.NewReaderSize(io.LimitReader(f, 64<<20), (2<<20)+1)
	events := []MemoryEvent{}
	previous := ""
	for {
		line, e := reader.ReadSlice('\n')
		if errors.Is(e, bufio.ErrBufferFull) {
			return events, errors.New("journal.memory_log_limit")
		}
		if e == io.EOF && len(line) == 0 {
			return events, nil
		}
		if e != nil {
			return events, ErrMemoryTornTail
		}
		if len(line) > 2<<20 || len(events) >= 1024 {
			return events, errors.New("journal.memory_log_limit")
		}
		var event MemoryEvent
		if json.Unmarshal(line, &event) != nil || event.Sequence != len(events)+1 || event.Previous != previous || event.Checksum != memoryEventHash(event) {
			return events, errors.New("journal.memory_log_corrupt")
		}
		events = append(events, event)
		previous = event.Checksum
	}
}

// RecoverMemoryTail requires the owning connection's driver lease. A partial
// final write had not committed, so no subsequent external effect was allowed.
// Preserve every original byte before restoring the already verified prefix.
// A malformed complete event, empty prefix or broken hash chain is never repaired.
func RecoverMemoryTail(ctx context.Context, path string) error {
	events, err := ReadMemoryLog(path)
	if err == nil {
		return nil
	}
	if !errors.Is(err, ErrMemoryTornTail) || len(events) == 0 {
		return err
	}
	b, err := os.ReadFile(path)
	if err != nil {
		return err
	}
	if len(b) > 64<<20 {
		return errors.New("journal.memory_log_limit")
	}
	end := bytes.LastIndexByte(b, '\n') + 1
	if end < 1 || end == len(b) {
		return errors.New("journal.memory_log_corrupt")
	}
	digest := sha256.Sum256(b)
	if err = vault.ReplaceFile(ctx, fmt.Sprintf("%s.torn-%x", path, digest[:8]), b); err != nil {
		return err
	}
	return vault.ReplaceFile(ctx, path, b[:end])
}

// AppendMemoryEvent is called under the connection driver/brief journal lock.
// The durable log precedes side effects; snapshot files are only conveniences.
func AppendMemoryEvent(ctx context.Context, path, kind string, value any) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	events, err := ReadMemoryLog(path)
	if err != nil {
		return err
	}
	data, err := json.Marshal(value)
	if err != nil {
		return err
	}
	event := MemoryEvent{Sequence: len(events) + 1, Kind: kind, Data: data}
	if len(events) > 0 {
		event.Previous = events[len(events)-1].Checksum
	}
	event.Checksum = memoryEventHash(event)
	b, err := json.Marshal(event)
	if err != nil {
		return err
	}
	if len(b) > 2<<20 || len(events) >= 1024 {
		return errors.New("journal.memory_log_limit")
	}
	b = append(b, '\n')
	if info, statErr := os.Stat(path); statErr == nil {
		if info.Size()+int64(len(b)) > 64<<20 {
			return errors.New("journal.memory_log_limit")
		}
	} else if !errors.Is(statErr, os.ErrNotExist) {
		return statErr
	}
	if err = os.MkdirAll(filepath.Dir(path), 0700); err != nil {
		return err
	}
	file, err := os.OpenFile(path, os.O_CREATE|os.O_APPEND|os.O_WRONLY, 0600)
	if err != nil {
		return err
	}
	n, writeErr := file.Write(b)
	if n != len(b) && writeErr == nil {
		writeErr = io.ErrShortWrite
	}
	syncErr := file.Sync()
	closeErr := file.Close()
	return errors.Join(writeErr, syncErr, closeErr)
}

// CheckpointMemoryLog requires the driver lease. Archive first, then atomically
// replace the active segment: a crash leaves either the entire old log or a
// checkpoint that names its already durable predecessor. Never discard history.
func CheckpointMemoryLog(ctx context.Context, path, expected string, value any) error {
	events, err := ReadMemoryLog(path)
	if err != nil {
		return err
	}
	if len(events) == 0 || events[len(events)-1].Checksum != expected {
		return errors.New("journal.memory_checkpoint_conflict")
	}
	original, err := os.ReadFile(path)
	if err != nil {
		return err
	}
	archive := filepath.Join(path+".history", expected+".jsonl")
	if info, e := os.Lstat(archive); e == nil {
		if !info.Mode().IsRegular() {
			return errors.New("journal.memory_checkpoint_conflict")
		}
		prior, e := os.ReadFile(archive)
		if e != nil {
			return e
		}
		if !bytes.Equal(prior, original) {
			return errors.New("journal.memory_checkpoint_conflict")
		}
	} else if !errors.Is(e, os.ErrNotExist) {
		return e
	} else if err = vault.ReplaceFile(ctx, archive, original); err != nil {
		return err
	}
	data, err := json.Marshal(value)
	if err != nil {
		return err
	}
	event := MemoryEvent{Sequence: 1, Kind: "checkpoint", Data: data}
	event.Checksum = memoryEventHash(event)
	b, err := json.Marshal(event)
	if err != nil {
		return err
	}
	if len(b) > 2<<20 {
		return errors.New("journal.memory_log_limit")
	}
	return vault.ReplaceFile(ctx, path, append(b, '\n'))
}
