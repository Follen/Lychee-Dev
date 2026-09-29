package protocol_test

import (
	"bufio"
	"context"
	"errors"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/live/channel"
	"github.com/follenfang/lycheedev/internal/live/memory"
)

type slotPeer struct {
	t       *testing.T
	dir     string
	in      io.WriteCloser
	out     *bufio.Scanner
	current bridge.SlotEnvelope
	fault   string
	sends   int
	driver  *channel.Driver
}

func (p *slotPeer) Observe(ctx context.Context, q channel.ObservationQuery) (channel.Observation, error) {
	return channel.ObserveRecords(ctx, p, q)
}

func (p *slotPeer) Publish(ctx context.Context, e bridge.SlotEnvelope) error {
	b, err := bridge.SlotPayload(e)
	if err != nil {
		return err
	}
	p.current = e
	return os.WriteFile(filepath.Join(p.dir, "payload.lua"), b, 0600)
}
func (p *slotPeer) send() error {
	p.sends++
	if p.current.Action == "release" {
		if _, err := os.Stat(filepath.Join(p.driver.ResultDir, p.driver.State.Operation.ID+".json")); err != nil {
			return errors.New("release before durable result")
		}
	}
	if _, err := fmt.Fprintf(p.in, "%d|%s\n", p.current.Index, filepath.ToSlash(filepath.Join(p.dir, "payload.lua"))); err != nil {
		return err
	}
	if !p.out.Scan() || p.out.Text() != "done" {
		return errors.New("lua peer failed")
	}
	if p.fault == p.current.Action {
		p.fault = ""
		return errors.New("injected host death after input")
	}
	return nil
}
func (p *slotPeer) Input(_ context.Context, a channel.InputAction) (channel.InputOutcome, error) {
	if a.Kind != "invoke" {
		return channel.InputOutcome{Disposition: "not_sent"}, errors.New("unexpected simulated input")
	}
	err := p.send()
	out := channel.InputOutcome{Disposition: "submitted", MessagesQueued: 1}
	if err != nil {
		out.Disposition = "uncertain"
	}
	return out, err
}
func (p *slotPeer) ObserveInput(_ context.Context, e bridge.SlotEnvelope, _ int64, _ string) (channel.InputObservation, error) {
	blocked := false
	return channel.InputObservation{Runtime: e.Runtime, Owner: e.Owner, Fence: e.Fence, NextSlot: e.Index, GUID: e.GUID, Build: e.Build, InputBlocked: &blocked}, nil
}
func (p *slotPeer) RuntimeCandidate(context.Context, channel.Identity) (*channel.Identity, error) {
	return nil, nil
}
func (p *slotPeer) Supersede(context.Context, bridge.SlotEnvelope, channel.Identity) error {
	return errors.New("unexpected simulated retirement")
}
func (p *slotPeer) Consumed(context.Context, bridge.SlotEnvelope) error { return nil }

type recordSource struct{ bytes []byte }

func (s recordSource) Verify(context.Context) error { return nil }
func (s recordSource) Regions(context.Context) ([]memory.Region, error) {
	return []memory.Region{{Range: memory.Range{Start: 4096, End: 4096 + uint64(len(s.bytes))}, Private: true, Committed: true, Readable: true}}, nil
}
func (s recordSource) Read(_ context.Context, address uint64, b []byte) (int, error) {
	if address < 4096 || address-4096 >= uint64(len(s.bytes)) {
		return 0, io.EOF
	}
	n := copy(b, s.bytes[address-4096:])
	if n < len(b) {
		return n, io.EOF
	}
	return n, nil
}
func (p *slotPeer) Find(ctx context.Context, s memory.Selector, first bool) (memory.LookupResult, error) {
	paths, err := filepath.Glob(filepath.Join(p.dir, "record-*.bin"))
	if err != nil {
		return memory.LookupResult{}, err
	}
	var data []byte
	for _, path := range paths {
		b, err := os.ReadFile(path)
		if err != nil {
			return memory.LookupResult{}, err
		}
		data = append(data, b...)
		data = append(data, make([]byte, 31)...)
	}
	return memory.Find(ctx, recordSource{data}, s, nil, first)
}
func TestSlotDriverLuaRecoveryWithoutBusinessReplay(t *testing.T) {
	for _, fault := range []string{"", "bind", "prepare", "commit", "confirm", "release", "reload_bind", "compile_error", "cleanup_error"} {
		t.Run("after_"+fault, func(t *testing.T) {
			ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
			defer cancel()
			dir := t.TempDir()
			root, _ := filepath.Abs(filepath.Join("..", "..", "addon"))
			runtimeGeneration := "1"
			if fault == "reload_bind" {
				runtimeGeneration = "2"
			}
			command := exec.CommandContext(ctx, luaRuntime(t), "slot_server.lua", filepath.ToSlash(root), filepath.ToSlash(dir), runtimeGeneration, fault)
			in, err := command.StdinPipe()
			if err != nil {
				t.Fatal(err)
			}
			stdout, err := command.StdoutPipe()
			if err != nil {
				t.Fatal(err)
			}
			var stderr strings.Builder
			command.Stderr = &stderr
			if err = command.Start(); err != nil {
				t.Fatal(err)
			}
			defer func() {
				fmt.Fprintln(in, "quit")
				in.Close()
				if err := command.Wait(); err != nil {
					t.Errorf("peer: %v %s", err, stderr.String())
				}
			}()
			peer := &slotPeer{t: t, dir: dir, in: in, out: bufio.NewScanner(stdout), fault: fault}
			if !peer.out.Scan() || peer.out.Text() != "ready" {
				t.Fatal("peer not ready")
			}
			identity := channel.Identity{Schema: "lycheedev.slot.identity.v1", Runtime: strings.Repeat("1", 32), NextSlot: 1, Slots: 64, GUID: "Player-1-123", Character: "Tester", Realm: "Realm", Build: "70000", Product: "retail", Release: "2.5.1"}
			path := filepath.Join(dir, "connections", "connection.jsonl")
			d, err := channel.New(path, peer, identity)
			if err != nil {
				t.Fatal(err)
			}
			peer.driver = d
			if err = d.Save(ctx, "created"); err != nil {
				t.Fatal(err)
			}
			restart := func() {
				var err error
				d, err = channel.Load(path, peer)
				if err != nil {
					t.Fatal(err)
				}
				peer.driver = d
			}
			if err = d.Connect(ctx); err != nil {
				if fault != "bind" && fault != "reload_bind" {
					t.Fatal(err)
				}
				restart()
				if fault == "reload_bind" {
					err = d.RecoverBinding(ctx)
				} else {
					err = d.Connect(ctx)
				}
				if err != nil {
					t.Fatal(err)
				}
			}
			// Lua decimal escapes must preserve Unicode, newlines, delimiters and NUL.
			code := "return { message = \"荔枝\\n\\000123\", quoted = [=[x\\y]=] }"
			if fault == "compile_error" {
				code = "return { broken ="
			}
			if err = d.PrepareOperation(ctx, code, 10, "observation"); err != nil {
				t.Fatal(err)
			}
			if err = d.Run(ctx); err != nil {
				if fault == "cleanup_error" {
					if !errors.Is(err, channel.ErrCleanupReloadRequired) || d.State.Operation.Stage != "release_ready" || d.State.Operation.CleanupMethod != "reload_required" {
						t.Fatal("lost verified report on cleanup failure", err)
					}
					sends := peer.sends
					restart()
					if e := d.Run(ctx); !errors.Is(e, channel.ErrCleanupReloadRequired) || peer.sends != sends {
						t.Fatal("repeated uncertain cleanup", e)
					}
					return
				}
				if fault == "" || fault == "bind" {
					t.Fatal(err)
				}
				restart()
				if err = d.Run(ctx); err != nil {
					t.Fatal(err)
				}
			}
			wantText, wantExecutions := "荔枝", "1"
			if fault == "compile_error" {
				wantText, wantExecutions = "compile_error", "0"
			}
			if d.State.Operation.Stage != "complete" || !strings.Contains(string(d.State.Operation.Result), wantText) {
				t.Fatalf("%+v", d.State.Operation)
			}
			count, err := os.ReadFile(filepath.Join(dir, "executions.txt"))
			if err != nil || string(count) != wantExecutions {
				t.Fatalf("executions=%s %v", count, err)
			}
			sends := peer.sends
			restart()
			if err = d.Run(ctx); err != nil || peer.sends != sends {
				t.Fatal("completed retry sent input", err)
			}
			if err = d.Disconnect(ctx); err != nil {
				t.Fatal(err)
			}
		})
	}
}
