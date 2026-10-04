package duplex

import (
	"bytes"
	"encoding/hex"
	"strings"
	"testing"
)

func reloadWireFixture(t *testing.T, kind Kind) Message {
	t.Helper()
	m := oneCommandFixture(t, nil)
	m.Header.Kind = kind
	m.Header.FrameIndex, m.Header.FrameCount = 0, 0
	m.Header.RequestID, m.Header.RequestSHA256 = strings.Repeat("0", 32), strings.Repeat("0", 64)
	m.Header.RequestSeq, m.Header.BudgetMillis, m.Header.TotalBytes = 0, 0, 0
	if kind == Lease {
		m.Payload = bytes.Repeat([]byte{0x66}, 16)
		m.Header.Challenge = strings.Repeat("7", 32)
	}
	return m
}

func TestReloadLeaseWireExactShapeAndIntegrity(t *testing.T) {
	for _, kind := range []Kind{Reload, Lease} {
		m := reloadWireFixture(t, kind)
		wire, err := EncodeMessage(m)
		if err != nil {
			t.Fatal(err)
		}
		got, err := DecodeMessage(wire)
		if err != nil || got.Header.Kind.Lane() != "stop" || !bytes.Equal(got.Payload, m.Payload) {
			t.Fatal(got, err)
		}
		if kind == Lease && hex.EncodeToString(got.Payload) != strings.Repeat("66", 16) {
			t.Fatal("lease lost original reload ID")
		}
		for _, change := range []func(*Message){
			func(x *Message) { x.Header.RequestID = strings.Repeat("6", 32) },
			func(x *Message) { x.Header.RequestSHA256 = strings.Repeat("6", 64) },
			func(x *Message) { x.Header.RequestSeq = 1 },
			func(x *Message) { x.Header.BudgetMillis = 1 },
			func(x *Message) { x.Header.TotalBytes = 1 },
			func(x *Message) { x.Payload = append(bytes.Clone(x.Payload), 0) },
		} {
			bad := m
			change(&bad)
			if _, err := EncodeMessage(bad); err == nil {
				t.Fatal("reload business/payload shape accepted", kind)
			}
		}
		if kind == Lease {
			wire[len(wire)-1] ^= 1
			if _, err := DecodeMessage(wire); err == nil {
				t.Fatal("lease ID corruption accepted")
			}
		}
	}
}

func TestReloadProjectionRequiresPrivatePreparationAndQuiescence(t *testing.T) {
	s := fixtureSendbox()
	s.Ready, s.BusinessReady = false, false
	s.ReadyChallenge = ""
	s.Reload = &ReloadPreparation{MessageID: strings.Repeat("6", 32), Challenge: strings.Repeat("7", 32)}
	if _, err := EncodeSendbox(s); err != nil {
		t.Fatal(err)
	}
	for _, change := range []func(*Sendbox){
		func(x *Sendbox) {
			x.Reload = &ReloadPreparation{MessageID: strings.Repeat("0", 32), Challenge: strings.Repeat("7", 32)}
		},
		func(x *Sendbox) {
			x.Reload = &ReloadPreparation{MessageID: strings.Repeat("6", 32), Challenge: strings.Repeat("0", 32)}
		},
		func(x *Sendbox) { x.ResourcesReleased = false },
		func(x *Sendbox) { x.Ready = true; x.BusinessReady = true; x.ReadyChallenge = strings.Repeat("b", 32) },
	} {
		bad := s
		change(&bad)
		if _, err := EncodeSendbox(bad); err == nil {
			t.Fatal("invalid reload preparation projected ready")
		}
	}
}
