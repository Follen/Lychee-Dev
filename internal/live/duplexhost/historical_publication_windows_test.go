//go:build windows && amd64

package duplexhost

// Retained historical stopped-publication trace tests; production only exposes
// the direct publisher. This adapter is intentionally compiled only in tests.
import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"github.com/follenfang/lycheedev/internal/buildinfo"
	"github.com/follenfang/lycheedev/internal/live"
	"github.com/follenfang/lycheedev/internal/live/duplex"
	"github.com/follenfang/lycheedev/internal/live/memory"
	"os"
	"path/filepath"
)

type historicalRowPublisher func(context.Context, memory.StoppedPublicationRequest) (duplex.WriteOutcome, []memory.DuplexWriteRange, memory.StoppedObservation, error)

// publishJournaled syncs the exact intent before the selected typed writer
// runs. Neither a complete WPM nor its readback proves addon execution.
func (n *Native) publishHistoricalJournaled(ctx context.Context, m duplex.Message, binding memory.LuaRootBinding, actorGUID, mode string, publish historicalRowPublisher) (out duplex.WriteOutcome, returned error) {
	out.State = duplex.NoWrite
	if mode != "direct" && mode != "stopped" {
		return out, errors.New("live.duplex_write_mode_invalid")
	}
	if n.TraceDir == "" {
		return out, errors.New("live.duplex_write_journal_required")
	}
	if err := os.MkdirAll(n.TraceDir, 0700); err != nil {
		return out, errors.Join(duplex.ErrPersistence, err)
	}
	file, err := os.OpenFile(filepath.Join(n.TraceDir, fmt.Sprintf("%s-%d.jsonl", m.Header.MessageID, m.Header.PublicationSeq)), os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
	if err != nil {
		return out, errors.Join(duplex.ErrPersistence, err)
	}
	defer func() {
		if e := file.Close(); e != nil {
			returned = errors.Join(returned, duplex.ErrPersistence, e)
		}
	}()
	enc := json.NewEncoder(file)
	// Command bytes are stored once in the coordinator journal. This immutable
	// reference and exact header are synced before starting the native helper.
	header := m.Header
	intent := struct {
		Mode          string                `json:"mode"`
		Header        duplex.Header         `json:"header"`
		PayloadSHA256 string                `json:"payloadSHA256"`
		PayloadBytes  int                   `json:"payloadBytes"`
		Root          memory.LuaRootBinding `json:"root"`
		ActorGUID     string                `json:"actorGUID"`
		Target        live.ClientWindow     `json:"target"`
	}{mode, header, digestBytes(m.Payload), len(m.Payload), binding, actorGUID, n.Target}
	if err = enc.Encode(intent); err != nil {
		return out, errors.Join(duplex.ErrPersistence, err)
	}
	if err = file.Sync(); err != nil {
		return out, errors.Join(duplex.ErrPersistence, err)
	}
	request := memory.StoppedPublicationRequest{
		Target:           memory.ProcessIdentity{PID: n.Target.Window.ProcessID, Created: n.Target.Window.ProcessStartedAt, Image: n.Target.Window.Executable},
		ExecutableSHA256: binding.ExecutableSHA256, Build: n.Target.Client.FullBuild, Product: n.Target.Client.Product, Release: buildinfo.Version, ActorGUID: actorGUID, Message: m,
	}
	out, facts, stop, writeErr := publish(ctx, request)
	// No disk wait occurs inside the native write critical section.
	fact := struct {
		Mode    string                    `json:"mode"`
		Outcome duplex.WriteOutcome       `json:"outcome"`
		Ranges  []memory.DuplexWriteRange `json:"ranges"`
		Stop    memory.StoppedObservation `json:"stop"`
		Error   string                    `json:"error,omitempty"`
	}{Mode: mode, Outcome: out, Ranges: facts, Stop: stop}
	if writeErr != nil {
		fact.Error = writeErr.Error()
	}
	if err = enc.Encode(fact); err == nil {
		err = file.Sync()
	}
	if err != nil {
		return out, errors.Join(writeErr, duplex.ErrPersistence, err)
	}
	return out, writeErr
}
