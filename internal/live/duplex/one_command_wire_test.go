package duplex

import (
	"bytes"
	"strings"
	"testing"
)

func oneCommandFixture(t *testing.T, source []byte) Message {
	t.Helper()
	id := Identity{Runtime: strings.Repeat("1", 32), Arena: strings.Repeat("2", 32), Session: strings.Repeat("3", 32), Owner: strings.Repeat("4", 32), ActorBinding: strings.Repeat("5", 32), Fence: 1}
	commands, err := NewFrames(id, strings.Repeat("6", 32), 1, 1, 1234, 1000, source)
	if err != nil || len(commands) != 1 {
		t.Fatal(err, len(commands))
	}
	return commands[0]
}

func TestOneCommandPreviousAckAndBodyIntegrity(t *testing.T) {
	m := oneCommandFixture(t, []byte("return true"))
	ack := ResultManifest{RequestID: strings.Repeat("6", 32), RequestSHA256: strings.Repeat("7", 64), State: "cancelled", SHA256: strings.Repeat("8", 64), Bytes: 12345, Pages: 1}
	digest, err := ResultAckSHA256(ack)
	if err != nil || digest != "5d6e3061959b6f9112cf02ed5e52b7d9d54f1d03f45e9f0dc9304e620e572317" {
		t.Fatal("independent Python hashlib vector mismatch", digest, err)
	}
	m.Header.PreviousResultAckSHA = digest
	wire, err := EncodeMessage(m)
	if err != nil {
		t.Fatal(err)
	}
	got, err := DecodeMessage(wire)
	if err != nil || got.Header.PreviousResultAckSHA != digest || got.Header.ControlSHA256 != "" {
		t.Fatal(got.Header, err)
	}
	for _, at := range []int{232, 320, len(wire) - 1} {
		bad := bytes.Clone(wire)
		bad[at] ^= 1
		if _, err := DecodeMessage(bad); err == nil {
			t.Fatalf("unchecked ACK/header/source mutation at %d", at)
		}
	}
	ack.Bytes++
	changed, err := ResultAckSHA256(ack)
	if err != nil || changed == digest {
		t.Fatal("ACK byte extent not bound", changed, err)
	}
	m.Header.PreviousResultAckSHA = ""
	wire, err = EncodeMessage(m)
	if err != nil || !bytes.Equal(wire[232:264], make([]byte, 32)) {
		t.Fatal("first command must carry zero ACK", err)
	}
}

func TestStopRowRetainsIndependentPayloadChecksum(t *testing.T) {
	m := oneCommandFixture(t, []byte("return true"))
	m.Header.Kind = Close
	m.Header.FrameIndex = 0
	m.Header.FrameCount = 0
	ack := ResultManifest{RequestID: m.Header.RequestID, RequestSHA256: m.Header.RequestSHA256, State: "cancelled", SHA256: strings.Repeat("8", 64), Bytes: 12345, Pages: 1}
	var err error
	m.Payload, err = EncodeResultAck(ack)
	if err != nil {
		t.Fatal(err)
	}
	wire, err := EncodeMessage(m)
	if err != nil {
		t.Fatal(err)
	}
	got, err := DecodeMessage(wire)
	if err != nil || got.Header.ControlSHA256 == "" || got.Header.PreviousResultAckSHA != strings.Repeat("0", 64) {
		t.Fatal(got.Header, err)
	}
	wire[len(wire)-1] ^= 1
	if _, err = DecodeMessage(wire); err == nil {
		t.Fatal("stop payload not authenticated")
	}
	m.Header.PreviousResultAckSHA = strings.Repeat("8", 64)
	if _, err := EncodeMessage(m); err == nil {
		t.Fatal("control carried command ACK union")
	}
}

func TestOneCommandRejectsMixedSnapshotsAndRetiredControls(t *testing.T) {
	old, err := EncodeMessage(oneCommandFixture(t, bytes.Repeat([]byte{'A'}, 96)))
	if err != nil {
		t.Fatal(err)
	}
	current, err := EncodeMessage(oneCommandFixture(t, bytes.Repeat([]byte{'B'}, 96)))
	if err != nil {
		t.Fatal(err)
	}
	for cut := 1; cut < len(old); cut++ {
		mixed := append(bytes.Clone(old[:cut]), current[cut:]...)
		if bytes.Equal(mixed, old) || bytes.Equal(mixed, current) {
			continue
		}
		if _, err := DecodeMessage(mixed); err == nil {
			t.Fatalf("mixed prefix publication accepted at %d", cut)
		}
	}
	for _, kind := range []Kind{Bind, Commit, ResultAck} {
		m := oneCommandFixture(t, nil)
		m.Header.Kind = kind
		m.Header.FrameIndex = 0
		m.Header.FrameCount = 0
		if _, err := EncodeMessage(m); err == nil {
			t.Fatalf("retired control accepted: %d", kind)
		}
	}
	m := oneCommandFixture(t, bytes.Repeat([]byte{1}, MaxSourceBytes))
	m.Header.FrameCount = 256
	if _, err := EncodeMessage(m); err == nil {
		t.Fatal("old segmented command accepted")
	}
}
