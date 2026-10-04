package protocol_test

import (
	"bytes"
	"context"
	"encoding/hex"
	"encoding/json"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/live/duplex"
)

func mailboxFixtureIdentity() duplex.Identity {
	return duplex.Identity{Runtime: "00112233445566778899aabbccddeeff", Arena: "10112233445566778899aabbccddeeff", Session: "20112233445566778899aabbccddeeff", Owner: "30112233445566778899aabbccddeeff", ActorBinding: "40112233445566778899aabbccddeeff", Fence: 11}
}

func writeGoCommand(t *testing.T, path string, source []byte) {
	t.Helper()
	commands, err := duplex.NewFrames(mailboxFixtureIdentity(), "50112233445566778899aabbccddeeff", 9007199254740993, 17, 7654321, 120000, source)
	if err != nil || len(commands) != 1 || commands[0].Header.FrameIndex != 1 || commands[0].Header.FrameCount != 1 {
		t.Fatal("command must use one publication", err)
	}
	wire, err := duplex.EncodeMessage(commands[0])
	if err != nil || len(wire) != duplex.HeaderBytes+len(source) {
		t.Fatal("complete command wire extent", err)
	}
	if err := os.WriteFile(path, wire, 0600); err != nil {
		t.Fatal(err)
	}
}

func TestDuplexGoLuaGolden(t *testing.T) {
	lua := luaRuntime(t)
	root, err := filepath.Abs("../../")
	if err != nil {
		t.Fatal(err)
	}
	tmp := t.TempDir()
	framePath, controlPath := filepath.Join(tmp, "command.bin"), filepath.Join(tmp, "stop.bin")
	reloadPath, leasePath := filepath.Join(tmp, "reload.bin"), filepath.Join(tmp, "lease.bin")
	inputPath, outputPath := filepath.Join(tmp, "sendbox.json"), filepath.Join(tmp, "sendbox.bin")
	writeGoCommand(t, framePath, []byte("return true"))
	id := mailboxFixtureIdentity()
	h := duplex.Header{Kind: duplex.Close, Runtime: id.Runtime, Arena: id.Arena, Session: id.Session, Owner: id.Owner, ActorBinding: id.ActorBinding, Fence: id.Fence,
		RequestID: strings.Repeat("0", 32), MessageID: "60112233445566778899aabbccddeeff", Challenge: strings.Repeat("0", 32), RequestSHA256: strings.Repeat("0", 64),
		PublicationSeq: 9, PublicationBegin: 18, PublicationEnd: 18, TransportAttempt: 1}
	control, err := duplex.EncodeMessage(duplex.Message{Header: h})
	if err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(controlPath, control, 0600); err != nil {
		t.Fatal(err)
	}
	h.Kind = duplex.Reload
	h.MessageID = "70112233445566778899aabbccddeeff"
	h.PublicationSeq, h.PublicationBegin, h.PublicationEnd = 10, 20, 20
	reload, err := duplex.EncodeMessage(duplex.Message{Header: h})
	if err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(reloadPath, reload, 0600); err != nil {
		t.Fatal(err)
	}
	original, _ := hex.DecodeString(h.MessageID)
	h.Kind = duplex.Lease
	h.MessageID = "80112233445566778899aabbccddeeff"
	h.Challenge = strings.Repeat("b", 32)
	h.PublicationSeq, h.PublicationBegin, h.PublicationEnd = 11, 22, 22
	lease, err := duplex.EncodeMessage(duplex.Message{Header: h, Payload: original})
	if err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(leasePath, lease, 0600); err != nil {
		t.Fatal(err)
	}
	box := duplex.Sendbox{Identity: id, ActorGUID: "Player-1-1", Character: "Paladin", Realm: "Realm", Build: "12.1.0.12345", Product: "retail", Release: "3.1.1",
		Schema: "lycheedev.mailbox.v1", LayoutID: duplex.MailboxLayoutID, Phase: "ready_unbound", ResourcesReleased: true, ActorReady: true, Ready: true, TransportReady: true, BusinessReady: true, ControlReady: true,
		ReadyChallenge: strings.Repeat("b", 32), AdmissionSequence: 1, StatusSequence: 1, Heartbeat: 1, Receipts: map[string]duplex.Receipt{}}
	jsonBody, err := json.Marshal(box)
	if err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(inputPath, jsonBody, 0600); err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Minute)
	defer cancel()
	output, err := exec.CommandContext(ctx, lua, "duplex.lua", root, framePath, controlPath, inputPath, outputPath, reloadPath, leasePath).CombinedOutput()
	if err != nil {
		t.Fatalf("%v\n%s", err, output)
	}
	if !bytes.Contains(output, []byte("Go wire, one write, exact u64, ACK, repair, close and fresh-owner fencing passed")) {
		t.Fatalf("missing golden result: %s", output)
	}
	wire, err := os.ReadFile(outputPath)
	if err != nil {
		t.Fatal(err)
	}
	got, err := duplex.DecodeSendbox(wire)
	if err != nil || got.Identity != box.Identity || got.ReadyChallenge != box.ReadyChallenge || got.AdmissionSequence != box.AdmissionSequence || got.Ready != got.BusinessReady {
		t.Fatalf("Lua sendbox changed Go contract: %+v error=%v", got, err)
	}
	t.Log(strings.TrimSpace(string(output)))
}

func TestDuplexLuaOneMiBAndRepeatedRelease(t *testing.T) {
	lua := luaRuntime(t)
	root, err := filepath.Abs("../../")
	if err != nil {
		t.Fatal(err)
	}
	largePath := filepath.Join(t.TempDir(), "command-one-mib.bin")
	source := []byte("return true" + strings.Repeat(" ", duplex.MaxSourceBytes-len("return true")))
	writeGoCommand(t, largePath, source)
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Minute)
	defer cancel()
	output, err := exec.CommandContext(ctx, lua, "duplex_stress.lua", root, largePath).CombinedOutput()
	if err != nil {
		t.Fatalf("%v\n%s", err, output)
	}
	if !bytes.Contains(output, []byte("1 MiB transfer and 140 released requests retained bounded memory")) {
		t.Fatalf("missing stress result: %s", output)
	}
	t.Log(strings.TrimSpace(string(output)))
}
