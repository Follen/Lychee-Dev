// Package slotset is an isolated admission model, NOT the production transport.
// It exercises finite fault assumptions before changing the Go/Lua protocol.
package slotset

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/vault"
)

const (
	maxConsumers = 4
	dataBytes    = (1 << 20) + 4096 // Lua decimal escaping expands 256 KiB by at most 4.
	controlBytes = 4096
	maxData      = 1
	maxControl   = 2 // One unknown old bind plus one fresh bind; NOT unlimited reload.
	modelSchema  = "lycheedev.slotset.prototype.v1"
)

var errQuota = errors.New("instance_quota")
var errAmbiguous = errors.New("route_ambiguous")

type member struct {
	Consumer string
	Envelope bridge.SlotEnvelope
	Digest   string
	Bytes    int
	Control  bool
}

type pool struct {
	Schema     string
	Generation uint64
	Consumers  map[string]bool
	Members    []member
}

func newPool() pool { return pool{Schema: modelSchema, Consumers: map[string]bool{}} }

func (p *pool) admit(consumer string) error {
	if consumer == "" {
		return errors.New("consumer_required")
	}
	if p.Consumers[consumer] {
		return nil
	}
	if len(p.Consumers) >= maxConsumers {
		return errQuota
	}
	p.Consumers[consumer] = true
	return nil
}

func digest(b []byte) string { h := sha256.Sum256(b); return hex.EncodeToString(h[:]) }

func (p *pool) append(consumer string, e bridge.SlotEnvelope) error {
	if !p.Consumers[consumer] {
		return errors.New("not_admitted")
	}
	b, err := bridge.SlotPayload(e)
	if err != nil {
		return err
	}
	control := e.Action != "prepare"
	limit := dataBytes
	if control {
		limit = controlBytes
	}
	if len(b) > limit {
		return errQuota
	}
	d := digest(b)
	data, controls := 0, 0
	for _, m := range p.Members {
		// Lua cannot validate the host's OS consumer. Known runtime collisions
		// must fail closed, including across different physical slot files.
		if m.Envelope.Runtime == e.Runtime && m.Consumer != consumer {
			return errAmbiguous
		}
		if m.Consumer == consumer {
			if m.Envelope.Nonce == e.Nonce {
				if m.Digest != d {
					return errors.New("immutable_member_changed")
				}
				return nil
			}
			if m.Control {
				controls++
			} else {
				data++
			}
		}
		if m.Envelope.Runtime == e.Runtime && m.Envelope.Index == e.Index {
			return errAmbiguous
		}
	}
	if control && controls >= maxControl || !control && data >= maxData {
		return errQuota
	}
	p.Members = append(p.Members, member{consumer, e, d, len(b), control})
	p.Generation++
	return nil
}

// The caller supplies already-verified exact evidence. This model does not
// claim that a consumer string or descriptor itself proves consumption.
func (p *pool) retire(consumer, runtime, nonce string) {
	for i, m := range p.Members {
		if m.Consumer == consumer && m.Envelope.Runtime == runtime && m.Envelope.Nonce == nonce {
			p.Members = append(p.Members[:i], p.Members[i+1:]...)
			p.Generation++
			return
		}
	}
}

func (p pool) verify(consumer string, e bridge.SlotEnvelope) bool {
	b, err := bridge.SlotPayload(e)
	if err != nil {
		return false
	}
	for _, m := range p.Members {
		if m.Consumer == consumer && m.Envelope.Nonce == e.Nonce {
			return m.Digest == digest(b)
		}
	}
	return false
}

type loader struct {
	runtime  string
	consumed [64]bool
}

func (l *loader) next() int {
	for i, v := range l.consumed {
		if !v {
			return i + 1
		}
	}
	return 65
}

// Models executing a valid generated Payload.lua once. A parser error before
// this callback is a separate production admission gate, not hidden here.
func (l *loader) wake(p pool) (*member, error) {
	i := l.next()
	if i > 64 {
		return nil, errors.New("exhausted")
	}
	l.consumed[i-1] = true // LoD execution cannot be undone, including no match.
	var match *member
	for _, m := range p.Members {
		if m.Envelope.Index == i && m.Envelope.Runtime == l.runtime {
			if match != nil {
				return nil, errAmbiguous
			}
			copy := m
			match = &copy
		}
	}
	if match == nil {
		return nil, errors.New("no_match_consumed")
	}
	return match, nil
}

func envelope(runtime, nonce int, action string, index int) bridge.SlotEnvelope {
	token := func(v int) string { return fmt.Sprintf("%032x", v) }
	return bridge.SlotEnvelope{Schema: bridge.SlotSchema, Index: index, Runtime: token(runtime), Nonce: token(nonce), Owner: token(800), Ticket: token(900), Fence: 1, Action: action, GUID: "Player-1", Build: "120100"}
}

func mustAppend(t *testing.T, p *pool, c string, e bridge.SlotEnvelope) {
	t.Helper()
	if err := p.append(c, e); err != nil {
		t.Fatal(err)
	}
}
func admitted(t *testing.T, consumers ...string) pool {
	t.Helper()
	p := newPool()
	for _, c := range consumers {
		if err := p.admit(c); err != nil {
			t.Fatal(err)
		}
	}
	return p
}

func TestIndependentConsumersAndRuntimeRecovery(t *testing.T) {
	p := admitted(t, "100/10", "200/20")
	a, b, newA := envelope(1, 1, "bind", 1), envelope(2, 2, "bind", 1), envelope(3, 3, "bind", 1)
	mustAppend(t, &p, "100/10", a)
	oldGeneration := p.Generation
	mustAppend(t, &p, "200/20", b)
	mustAppend(t, &p, "100/10", newA)
	if p.Generation == oldGeneration || !p.verify("100/10", a) {
		t.Fatal("file merge changed immutable old member")
	}
	for _, e := range []bridge.SlotEnvelope{a, b, newA} {
		l := loader{runtime: e.Runtime}
		m, err := l.wake(p)
		if err != nil || m.Envelope.Nonce != e.Nonce {
			t.Fatal("wrong routing", err)
		}
	}
	p.retire("100/10", a.Runtime, a.Nonce)
	// A late receipt only addresses its original member, never the new bind.
	p.retire("100/10", a.Runtime, a.Nonce)
	if !p.verify("100/10", newA) || !p.verify("200/20", b) {
		t.Fatal("foreign/new member retired")
	}
	changed := newA
	changed.Action = "unbind"
	if err := p.append("100/10", changed); err == nil {
		t.Fatal("same nonce mutated")
	}
}

func TestStableInstanceQuotaAndRecoveryReserve(t *testing.T) {
	p := admitted(t, "100/10", "200/20", "300/30", "400/40")
	if err := p.admit("500/50"); !errors.Is(err, errQuota) {
		t.Fatal("unbounded admission", err)
	}
	a := envelope(1, 1, "prepare", 1)
	a.Code = strings.Repeat("\xff", 262144)
	mustAppend(t, &p, "100/10", a)
	// A full data budget cannot consume the separately reserved bind quota.
	mustAppend(t, &p, "100/10", envelope(2, 2, "bind", 1))
	mustAppend(t, &p, "100/10", envelope(3, 3, "bind", 1))
	for r := 4; r < 100; r++ {
		if err := p.admit("100/10"); err != nil {
			t.Fatal(err)
		}
		if err := p.append("100/10", envelope(r, r, "bind", 1)); !errors.Is(err, errQuota) {
			t.Fatal("reload refreshed quota", r, err)
		}
	}
	for i, c := range []string{"200/20", "300/30", "400/40"} {
		base := 100 + i*10
		e := envelope(base, base, "prepare", 1)
		e.Code = a.Code
		mustAppend(t, &p, c, e)
		mustAppend(t, &p, c, envelope(base+1, base+1, "bind", 1))
		mustAppend(t, &p, c, envelope(base+2, base+2, "bind", 1))
	}
	total := 0
	for _, m := range p.Members {
		total += m.Bytes
	}
	if total > maxConsumers*(dataBytes+maxControl*controlBytes) {
		t.Fatal("byte budget exceeded")
	}
	t.Logf("worst-case fixture: %d immutable members, %d serialized Lua bytes (collection framing excluded)", len(p.Members), total)
}

func TestIdentityAmbiguityAndOneShotLoad(t *testing.T) {
	p := admitted(t, "100/10", "200/20")
	a := envelope(1, 1, "bind", 1)
	mustAppend(t, &p, "100/10", a)
	if err := p.append("200/20", envelope(1, 2, "bind", 2)); !errors.Is(err, errAmbiguous) {
		t.Fatal("cross-consumer collision accepted", err)
	}
	l := loader{runtime: envelope(2, 2, "bind", 1).Runtime}
	if _, err := l.wake(p); err == nil || l.next() != 2 {
		t.Fatal("unmatched LoD not consumed")
	}
	mustAppend(t, &p, "200/20", envelope(2, 2, "bind", 1))
	if m, _ := l.wake(p); m != nil || l.next() != 3 {
		t.Fatal("loaded slot reread")
	}
	// Corrupt or ambiguous input must consume once and never select first.
	q := newPool()
	q.Members = []member{p.Members[0], p.Members[0]}
	l = loader{runtime: a.Runtime}
	if _, err := l.wake(q); !errors.Is(err, errAmbiguous) || l.next() != 2 {
		t.Fatal("ambiguous route", err)
	}
	// A late wake still advances to the next physical slot. The prototype
	// deliberately exposes this limitation instead of pretending to address it.
	mustAppend(t, &p, "100/10", envelope(1, 3, "confirm", 2))
	l = loader{runtime: a.Runtime}
	_, _ = l.wake(p)
	m, err := l.wake(p)
	if err != nil || m.Envelope.Index != 2 {
		t.Fatal("late-wake fault model changed", err)
	}
}

type pending struct {
	Before string
	After  string
	Bytes  []byte
}

// Small write-ahead model using the existing atomic-file primitive. It is not
// the production two-file Lua/manifest publisher. All calls hold an imaginary
// installation lock; process interleavings are serialized explicitly below.
func publish(path string, p pool, cut string) error {
	old, err := os.ReadFile(path)
	if err != nil {
		return err
	}
	b, err := json.Marshal(p)
	if err != nil {
		return err
	}
	w, _ := json.Marshal(pending{digest(old), digest(b), b})
	if err = vault.ReplaceFile(context.Background(), path+".intent", w); err != nil {
		return err
	}
	if cut == "intent" {
		return nil
	}
	if err = vault.ReplaceFile(context.Background(), path, b); err != nil {
		return err
	}
	if cut == "payload" {
		return nil
	}
	return os.Remove(path + ".intent")
}

func recoverFile(path string) (pool, error) {
	var p pool
	current, err := os.ReadFile(path)
	if err != nil {
		return p, err
	}
	w, err := os.ReadFile(path + ".intent")
	if err == nil {
		var record pending
		if err = json.Unmarshal(w, &record); err != nil {
			return p, err
		}
		if digest(record.Bytes) != record.After {
			return p, errors.New("intent_corrupt")
		}
		if digest(current) != record.Before && digest(current) != record.After {
			return p, errors.New("unknown_file_bytes")
		}
		if err = vault.ReplaceFile(context.Background(), path, record.Bytes); err != nil {
			return p, err
		}
		current = record.Bytes
		if err = os.Remove(path + ".intent"); err != nil {
			return p, err
		}
	} else if !os.IsNotExist(err) {
		return p, err
	}
	err = json.Unmarshal(current, &p)
	return p, err
}

func TestMergeAndRetirementCrashBoundaries(t *testing.T) {
	for _, remove := range []bool{false, true} {
		for _, cut := range []string{"intent", "payload", "complete"} {
			t.Run(fmt.Sprintf("remove_%v_%s", remove, cut), func(t *testing.T) {
				p := admitted(t, "100/10", "200/20")
				a, b := envelope(1, 1, "bind", 1), envelope(2, 2, "bind", 1)
				mustAppend(t, &p, "100/10", a)
				if remove {
					mustAppend(t, &p, "200/20", b)
				}
				path := filepath.Join(t.TempDir(), "pool.json")
				raw, _ := json.Marshal(p)
				if err := vault.ReplaceFile(context.Background(), path, raw); err != nil {
					t.Fatal(err)
				}
				if remove {
					p.retire("100/10", a.Runtime, a.Nonce)
				} else {
					mustAppend(t, &p, "200/20", b)
				}
				if err := publish(path, p, cut); err != nil {
					t.Fatal(err)
				}
				for repeat := 0; repeat < 2; repeat++ {
					got, err := recoverFile(path)
					if err != nil {
						t.Fatal(err)
					}
					if !got.verify("200/20", b) || got.verify("100/10", a) == remove {
						t.Fatal("merge/retirement lost exact member")
					}
				}
			})
		}
	}
}

func TestUnknownBytesFailClosed(t *testing.T) {
	p := admitted(t, "100/10")
	path := filepath.Join(t.TempDir(), "pool.json")
	raw, _ := json.Marshal(p)
	if err := os.WriteFile(path, raw, 0600); err != nil {
		t.Fatal(err)
	}
	mustAppend(t, &p, "100/10", envelope(1, 1, "bind", 1))
	if err := publish(path, p, "intent"); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, []byte("foreign bytes"), 0600); err != nil {
		t.Fatal(err)
	}
	if _, err := recoverFile(path); err == nil {
		t.Fatal("overwrote unknown bytes")
	}
	b, _ := os.ReadFile(path)
	if string(b) != "foreign bytes" {
		t.Fatal("changed unknown file")
	}
}
