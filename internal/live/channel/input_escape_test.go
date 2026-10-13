package channel

import (
	"context"
	"errors"
	"path/filepath"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/bridge"
)

type focusedPeer struct {
	pendingBackend
	blocked bool
}

func (p *focusedPeer) ObserveInput(ctx context.Context, e bridge.SlotEnvelope, after int64, capability string) (InputObservation, error) {
	s, err := p.pendingBackend.ObserveInput(ctx, e, after, capability)
	s.InputBlocked = &p.blocked
	if p.blocked {
		s.Reason = "input_focus_unavailable"
	}
	return s, err
}
func TestFocusedInputWaitsWithoutEscapeAcrossResume(t *testing.T) {
	for _, action := range []string{"bind", "prepare", "commit", "confirm", "release", "unbind", "reload"} {
		t.Run(action, func(t *testing.T) {
			peer := &focusedPeer{blocked: true}
			i := Identity{Schema: IdentitySchema, Runtime: strings.Repeat("1", 32), Slots: 500, NextSlot: 1, GUID: "g", Character: "c", Realm: "r", Build: "b", Product: "retail", Release: "3.1.2", InputState: InputSchema}
			d, err := New(filepath.Join(t.TempDir(), "c.jsonl"), peer, i)
			if err != nil {
				t.Fatal(err)
			}
			if err = d.Save(context.Background(), "connection_created"); err != nil {
				t.Fatal(err)
			}
			e := bridge.SlotEnvelope{Runtime: i.Runtime, Owner: d.State.Owner, Fence: 1, Index: 1, GUID: i.GUID, Build: i.Build, Action: action}
			desired := "invoke"
			if action == "reload" {
				desired = "reload"
			}
			for retry := 0; retry < 4; retry++ {
				err = d.submitInput(context.Background(), "same-exchange", i.Runtime, desired, e)
				if !errors.Is(err, ErrPending) || peer.sends != 0 || d.State.Input != nil {
					t.Fatalf("focus caused input: %v %+v", err, d.State.Input)
				}
				d, err = Load(d.Log, peer)
				if err != nil {
					t.Fatal(err)
				}
			}
			peer.blocked = false
			if err = d.submitInput(context.Background(), "same-exchange", i.Runtime, desired, e); err != nil || peer.sends != 1 {
				t.Fatal(err, peer.sends)
			}
			d.State.Input.Outcome = nil
			if err = d.Save(context.Background(), "input_intent"); err != nil {
				t.Fatal(err)
			}
			d, err = Load(d.Log, peer)
			if err != nil {
				t.Fatal(err)
			}
			if err = d.submitInput(context.Background(), "same-exchange", i.Runtime, desired, e); err != nil || peer.sends != 1 {
				t.Fatal("unknown input replayed", err, peer.sends)
			}
		})
	}
}
func TestCurrentProtocolRejectsEscapesAndLegacyInput(t *testing.T) {
	blocked := true
	e := bridge.SlotEnvelope{Runtime: strings.Repeat("1", 32), Owner: "o", Fence: 1, Index: 1, GUID: "g", Build: "b", Action: "reload"}
	s := InputObservation{Schema: InputSchema, Runtime: e.Runtime, Owner: e.Owner, Fence: 1, NextSlot: 1, GUID: "g", Build: "b", SampleMillis: 1200, InputBlocked: &blocked, Reason: "input_focus_unavailable", Bindings: primaryInputBindings()}
	for _, limit := range []int{-1, 0, 1, 2, 3, 4} {
		s.EscapeLimit = limit
		_, err := inputObservation(inputTestRecord(t, s), e, 1000, 1250)
		if (err == nil) != (limit == 0) {
			t.Fatalf("escape limit %d: %v", limit, err)
		}
	}
	s.EscapeLimit = 0
	s.Schema = legacyInputSchema
	if _, err := inputObservation(inputTestRecord(t, s), e, 1000, 1250); err == nil {
		t.Fatal("legacy record granted reload")
	}
}
