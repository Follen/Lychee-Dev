package publish

import (
	"bytes"
	"context"
	"encoding/binary"
	"net"
	"net/url"
	"sync/atomic"
	"testing"
	"time"
)

type ddSessionTestAPI struct{ calls, closes atomic.Int32 }

func (a *ddSessionTestAPI) identity() map[string]any {
	return map[string]any{"author_id": "original-author", "active": true}
}
func (a *ddSessionTestAPI) call(ctx context.Context, path string, q url.Values, body any, mutation bool) (map[string]any, error) {
	a.calls.Add(1)
	if path == "/slow" {
		<-ctx.Done()
		return nil, ctx.Err()
	}
	return map[string]any{"code": 200, "result": map[string]any{"path": path, "mutation": mutation}}, nil
}
func (a *ddSessionTestAPI) channels(context.Context) (any, error) {
	return map[string]any{"items": []any{}}, nil
}
func (a *ddSessionTestAPI) close() error { a.closes.Add(1); return nil }
func TestDDSessionFrameRejectsOversizeAndDuplicateInput(t *testing.T) {
	raw := binary.LittleEndian.AppendUint32(nil, ddSessionLimit+1)
	var v any
	if ddReadFrame(bytes.NewReader(raw), &v) == nil {
		t.Fatal("accepted oversized frame")
	}
	raw = binary.LittleEndian.AppendUint32(nil, 13)
	raw = append(raw, []byte(`{"a":1,"a":2}`)...)
	if ddReadFrame(bytes.NewReader(raw), &v) == nil {
		t.Fatal("accepted duplicate JSON")
	}
}
func TestDDSessionRPCAuthDeadlineAndNoMutationReplay(t *testing.T) {
	api := &ddSessionTestAPI{}
	state := ddSessionState{Nonce: "original-nonce"}
	exchange := func(call ddSessionCall) (ddSessionReply, bool) {
		client, server := net.Pipe()
		done := make(chan bool, 1)
		go func() { done <- ddServeSessionBounded(server, state, api, nil, nil); server.Close() }()
		client.SetDeadline(time.Now().Add(time.Second))
		ddWriteFrame(client, call)
		var reply ddSessionReply
		ddReadFrame(client, &reply)
		client.Close()
		return reply, <-done
	}
	call := ddSessionCall{Schema: ddSessionWire, Nonce: "other", ID: "1", Operation: "call", Path: "/write", Mutation: true, Deadline: time.Now().Add(time.Second).UnixMilli()}
	exchange(call)
	if api.calls.Load() != 0 {
		t.Fatal("unauthenticated call reached API")
	}
	call.Nonce = state.Nonce
	call.Deadline = time.Now().Add(-time.Second).UnixMilli()
	reply, _ := exchange(call)
	if api.calls.Load() != 0 || reply.Error == nil {
		t.Fatal("expired call reached API")
	}
	call.Deadline = time.Now().Add(time.Second).UnixMilli()
	reply, _ = exchange(call)
	if reply.Error != nil || api.calls.Load() != 1 {
		t.Fatal("valid mutation")
	}
	call.Path = "/slow"
	call.Deadline = time.Now().Add(30 * time.Millisecond).UnixMilli()
	exchange(call)
	if api.calls.Load() != 2 {
		t.Fatal("mutation replay")
	}
	call.Operation = "stop"
	call.Deadline = time.Now().Add(time.Second).UnixMilli()
	_, stopped := exchange(call)
	if !stopped || api.closes.Load() != 1 {
		t.Fatal("stop did not close owned runtime")
	}
}

func TestDDSessionCloseAcknowledgesOnlyPersistedState(t *testing.T) {
	for _, saveFails := range []bool{false, true} {
		api := &ddSessionTestAPI{}
		state := ddSessionState{Nonce: "original-nonce"}
		client, server := net.Pipe()
		done := make(chan bool, 1)
		var saved atomic.Bool
		go func() {
			done <- ddServeSessionBounded(server, state, api, nil, func() error {
				if api.closes.Load() != 1 {
					return failure("test.close_order", "runtime was not closed")
				}
				saved.Store(true)
				if saveFails {
					return failure("test.save", "save failed")
				}
				return nil
			})
			server.Close()
		}()
		client.SetDeadline(time.Now().Add(time.Second))
		call := ddSessionCall{Schema: ddSessionWire, Nonce: state.Nonce, ID: "close", Operation: "stop", Deadline: time.Now().Add(time.Second).UnixMilli()}
		if err := ddWriteFrame(client, call); err != nil {
			t.Fatal(err)
		}
		var reply ddSessionReply
		if err := ddReadFrame(client, &reply); err != nil {
			t.Fatal(err)
		}
		client.Close()
		stopped := <-done
		if !saved.Load() || (reply.Error != nil) != saveFails || stopped == saveFails {
			t.Fatalf("saveFails=%v saved=%v stopped=%v reply=%#v", saveFails, saved.Load(), stopped, reply)
		}
	}
}
