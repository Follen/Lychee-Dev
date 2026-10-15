package bridge

import (
	"bytes"
	"github.com/follenfang/lycheedev/internal/buildinfo"
	"os"
	"reflect"
	"strings"
	"testing"
)

func queueFixture(id string) ProbeDefinition {
	return ProbeDefinition{RequestID: id, Release: buildinfo.Version, SessionNonce: strings.Repeat("a", 32), ReloadNonce: strings.Repeat("b", 32), Character: "Paladin", Realm: "Realm", GUID: "Player-1-123", Product: "retail", Build: "12.1.0.12345", Code: "return { text = '世界\\\"\n' }"}
}
func TestProbeQueueCodec(t *testing.T) {
	a, b := queueFixture("OP-a"), queueFixture("OP-b")
	source, err := EncodeProbeQueue([]ProbeDefinition{b, a})
	if err != nil {
		t.Fatal(err)
	}
	decoded, err := DecodeProbeQueue(bytes.NewReader(source))
	if err != nil || !reflect.DeepEqual(decoded, []ProbeDefinition{a, b}) {
		t.Fatalf("%+v %v", decoded, err)
	}
	for _, bad := range [][]byte{
		append(append([]byte(nil), source...), []byte("print('injected')")...),
		bytes.Replace(source, []byte("codeBytes="), []byte("extra=true,codeBytes="), 1),
		bytes.Replace(source, []byte("codeSHA256=\""), []byte("codeSHA256=\"0"), 1),
		bytes.Replace(source, []byte("codeAdler32=\""), []byte("codeAdler32=\"0"), 1),
		bytes.Replace(source, []byte("lycheedev.queue.v1"), []byte("lycheedev.queue.v2"), 1),
		bytes.Replace(source, []byte("codeBytes="), []byte("codeBytes=1,unused="), 1),
		[]byte("return os.execute('no')"),
	} {
		if _, err := DecodeProbeQueue(bytes.NewReader(bad)); err == nil {
			t.Fatal("modified queue accepted")
		}
	}
	if _, err := DecodeProbeQueue(strings.NewReader(strings.Repeat("x", MaxProbeQueueFileBytes+1))); err == nil {
		t.Fatal("oversized queue accepted")
	}
	if _, err := EncodeProbeQueue([]ProbeDefinition{a, a}); err == nil {
		t.Fatal("duplicate accepted")
	}
	for _, change := range []func(*ProbeDefinition){
		func(d *ProbeDefinition) { d.SessionNonce = "short" }, func(d *ProbeDefinition) { d.Product = "era" },
		func(d *ProbeDefinition) { d.RequestID = "injected\"]" }, func(d *ProbeDefinition) { d.Character = "" },
		func(d *ProbeDefinition) { d.Code = "\x1bLua" }, func(d *ProbeDefinition) { d.Code = "\x00" },
		func(d *ProbeDefinition) { d.Code = "\xff" }, func(d *ProbeDefinition) { d.Code = strings.Repeat("x", (256<<10)+1) },
	} {
		d := a
		change(&d)
		if _, err := EncodeProbeQueue([]ProbeDefinition{d}); err == nil {
			t.Fatal("invalid definition accepted")
		}
	}
	defs := make([]ProbeDefinition, 0, 17)
	for i := 0; i < 17; i++ {
		d := a
		d.RequestID = string(rune('a' + i))
		defs = append(defs, d)
	}
	if _, err := EncodeProbeQueue(defs); err == nil {
		t.Fatal("count limit ignored")
	}
	for i := range defs[:5] {
		defs[i].Code = strings.Repeat(" ", 256<<10)
	}
	if _, err := EncodeProbeQueue(defs[:5]); err == nil {
		t.Fatal("byte limit ignored")
	}
	if _, err := EncodeProbeQueue(defs[:4]); err != nil {
		t.Fatal(err)
	}
}
func TestShippedQueueEmpty(t *testing.T) {
	data, err := os.ReadFile("../../addon/Bridge/Definitions.lua")
	if err != nil {
		t.Fatal(err)
	}
	definitions, err := DecodeProbeQueue(bytes.NewReader(data))
	if err != nil || len(definitions) != 0 {
		t.Fatalf("release queue is not canonical and empty: %v", err)
	}
}

func TestAcknowledgementFieldRejected(t *testing.T) {
	source, err := EncodeProbeQueue([]ProbeDefinition{queueFixture("OP-ack")})
	if err != nil {
		t.Fatal(err)
	}
	legacy := bytes.Replace(source, []byte("codeBytes="), []byte("acknowledgement={},codeBytes="), 1)
	if _, err := DecodeProbeQueue(bytes.NewReader(legacy)); err == nil {
		t.Fatal("legacy acknowledgement field accepted")
	}
}
func FuzzProbeQueue(f *testing.F) {
	source, _ := EncodeProbeQueue([]ProbeDefinition{queueFixture("OP-seed")})
	f.Add(source)
	f.Fuzz(func(t *testing.T, data []byte) {
		if len(data) > 6<<20 {
			return
		}
		definitions, err := DecodeProbeQueue(bytes.NewReader(data))
		if err != nil {
			return
		}
		encoded, err := EncodeProbeQueue(definitions)
		if err != nil || !bytes.Equal(encoded, data) {
			t.Fatal("accepted noncanonical queue")
		}
	})
}
