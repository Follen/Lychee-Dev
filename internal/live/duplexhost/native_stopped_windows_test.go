//go:build windows && amd64

package duplexhost

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/buildinfo"
	"github.com/follenfang/lycheedev/internal/live/duplex"
	"github.com/follenfang/lycheedev/internal/live/memory"
)

func stoppedMessageFixture(t *testing.T) duplex.Message {
	t.Helper()
	frames, err := duplex.NewFrames(hostIdentity, strings.Repeat("6", 32), 1, 1, 1234, 1000, []byte("return 42"))
	if err != nil {
		t.Fatal(err)
	}
	return frames[0]
}

func TestStoppedHostPublicationDurabilityAndExactPins(t *testing.T) {
	writeErr := errors.New("fixture helper failed after publication")
	for _, tc := range []struct {
		name string
		out  duplex.WriteOutcome
		err  error
	}{
		{"complete", duplex.WriteOutcome{State: duplex.CompleteWrite, Bytes: 6293376, ReadbackVerified: true}, nil},
		{"unknown", duplex.WriteOutcome{State: duplex.UnknownWrite}, writeErr},
		{"partial", duplex.WriteOutcome{State: duplex.PartialWrite, Bytes: 24}, writeErr},
		{"no_write", duplex.WriteOutcome{State: duplex.NoWrite}, writeErr},
	} {
		t.Run(tc.name, func(t *testing.T) {
			ctx, cancel := context.WithTimeout(context.Background(), time.Second)
			defer cancel()
			n := &Native{TraceDir: t.TempDir()}
			n.Target.Window.ProcessID = 7
			n.Target.Window.ProcessStartedAt = 9
			n.Target.Window.Executable = filepath.Join(t.TempDir(), "Wow.exe")
			n.Target.Client.FullBuild = "12.0.1.69933"
			n.Target.Client.Product = "wow"
			binding := memory.LuaRootBinding{ExecutableSHA256: strings.Repeat("a", 64), RootRVA: 88}
			actor := "Player-1-FIXTURE"
			message := stoppedMessageFixture(t)
			path := filepath.Join(n.TraceDir, fmt.Sprintf("%s-%d.jsonl", message.Header.MessageID, message.Header.PublicationSeq))
			ranges := []memory.DuplexWriteRange{{Phase: "write", ExpectedBytes: 6293376, Bytes: tc.out.Bytes, ImageSHA256: strings.Repeat("b", 64)}}
			stop := memory.StoppedObservation{DebuggerThread: 13, AttachMicros: 211, StoppedMicros: 321, Detached: true}
			calls := 0
			publish := func(gotCtx context.Context, request memory.StoppedPublicationRequest) (duplex.WriteOutcome, []memory.DuplexWriteRange, memory.StoppedObservation, error) {
				calls++
				if gotCtx != ctx || request.Target != (memory.ProcessIdentity{PID: 7, Created: 9, Image: n.Target.Window.Executable}) || request.ExecutableSHA256 != binding.ExecutableSHA256 || request.Build != n.Target.Client.FullBuild || request.Product != n.Target.Client.Product || request.Release != buildinfo.Version || request.ActorGUID != actor || !reflect.DeepEqual(request.Message, message) {
					t.Fatalf("helper received changed publication pins: %+v", request)
				}
				if request.Parent != (memory.ProcessIdentity{}) || request.Invocation != "" || request.DeadlineUnixNano != 0 {
					t.Fatal("host supplied helper-owned invocation identity")
				}
				// The intent must already be readable before any helper effect.
				body, err := os.ReadFile(path)
				if err != nil {
					t.Fatal("publication started before durable intent", err)
				}
				var intent struct {
					Header        duplex.Header         `json:"header"`
					PayloadSHA256 string                `json:"payloadSHA256"`
					PayloadBytes  int                   `json:"payloadBytes"`
					Root          memory.LuaRootBinding `json:"root"`
					ActorGUID     string                `json:"actorGUID"`
				}
				decoder := json.NewDecoder(bytes.NewReader(body))
				if err = decoder.Decode(&intent); err != nil || intent.Header != message.Header || intent.PayloadSHA256 != digestBytes(message.Payload) || intent.PayloadBytes != len(message.Payload) || !reflect.DeepEqual(intent.Root, binding) || intent.ActorGUID != actor {
					t.Fatal("intent did not pin exact publication", intent, err)
				}
				if err = decoder.Decode(new(any)); err != io.EOF {
					t.Fatal("outcome appeared before helper returned", err)
				}
				return tc.out, ranges, stop, tc.err
			}
			out, err := n.publishHistoricalJournaled(ctx, message, binding, actor, "stopped", publish)
			if out != tc.out || !errors.Is(err, tc.err) || calls != 1 {
				t.Fatal("helper outcome changed", out, err, calls)
			}
			body, err := os.ReadFile(path)
			if err != nil {
				t.Fatal(err)
			}
			decoder := json.NewDecoder(bytes.NewReader(body))
			if err = decoder.Decode(new(any)); err != nil {
				t.Fatal(err)
			}
			var fact struct {
				Outcome duplex.WriteOutcome       `json:"outcome"`
				Ranges  []memory.DuplexWriteRange `json:"ranges"`
				Stop    memory.StoppedObservation `json:"stop"`
				Error   string                    `json:"error"`
			}
			if err = decoder.Decode(&fact); err != nil || fact.Outcome != tc.out || !reflect.DeepEqual(fact.Ranges, ranges) || fact.Stop != stop {
				t.Fatal("helper facts were not saved exactly", fact, err)
			}
			if tc.err == nil && fact.Error != "" || tc.err != nil && fact.Error != tc.err.Error() {
				t.Fatal("helper error was not saved exactly", fact.Error)
			}
			if err = decoder.Decode(new(any)); err != io.EOF {
				t.Fatal("unexpected extra publication facts", err)
			}
			if tc.out.State == duplex.UnknownWrite {
				if _, err = n.publishHistoricalJournaled(ctx, message, binding, actor, "stopped", publish); !errors.Is(err, duplex.ErrPersistence) || calls != 1 {
					t.Fatal("uncertain publication replayed", calls, err)
				}
			}
		})
	}
}

func TestStoppedHostIntentFailurePreventsHelper(t *testing.T) {
	ctx := context.Background()
	message := stoppedMessageFixture(t)
	blocked := filepath.Join(t.TempDir(), "file")
	if err := os.WriteFile(blocked, nil, 0600); err != nil {
		t.Fatal(err)
	}
	for _, dir := range []string{"", filepath.Join(blocked, "trace")} {
		n := &Native{TraceDir: dir}
		out, err := n.publishHistoricalJournaled(ctx, message, memory.LuaRootBinding{}, "actor", "stopped", func(context.Context, memory.StoppedPublicationRequest) (duplex.WriteOutcome, []memory.DuplexWriteRange, memory.StoppedObservation, error) {
			t.Fatal("helper started without durable intent")
			return duplex.WriteOutcome{}, nil, memory.StoppedObservation{}, nil
		})
		if err == nil || out.State != duplex.NoWrite {
			t.Fatal("failed intent did not preserve no-write outcome", out, err)
		}
	}
}

func TestStoppedHostProductionGateDoesNotStartPublication(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	n := &Native{Mailbox: &readFixture{}, TraceDir: filepath.Join(t.TempDir(), "not-created")}
	out, err := n.Publish(ctx, stoppedMessageFixture(t))
	if err == nil || !strings.Contains(err.Error(), "writer_profile_unverified") || out.State != duplex.NoWrite {
		t.Fatal("unqualified writer reached publication", out, err)
	}
	if _, err = os.Stat(n.TraceDir); !errors.Is(err, os.ErrNotExist) {
		t.Fatal("unqualified writer started trace or helper", err)
	}
}
