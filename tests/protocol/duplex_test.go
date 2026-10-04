package protocol_test

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/live/duplex"
)

func TestDuplexGoLuaGolden(t *testing.T) {
	lua := luaRuntime(t)
	root, err := filepath.Abs("../../")
	if err != nil {
		t.Fatal(err)
	}
	tmp := t.TempDir()
	framePath := filepath.Join(tmp, "frame.bin")
	controlPath := filepath.Join(tmp, "control.bin")
	inputPath := filepath.Join(tmp, "sendbox.json")
	outputPath := filepath.Join(tmp, "sendbox.bin")
	id := duplex.Identity{Runtime: "00112233445566778899aabbccddeeff", Arena: "10112233445566778899aabbccddeeff", Session: "20112233445566778899aabbccddeeff", Owner: "30112233445566778899aabbccddeeff", ActorBinding: "40112233445566778899aabbccddeeff", Fence: 11}
	frames, err := duplex.NewFrames(id, "50112233445566778899aabbccddeeff", 9007199254740993, 17, 7654321, 120000, []byte("return true"))
	if err != nil {
		t.Fatal(err)
	}
	frame, err := duplex.EncodeMessage(frames[0])
	if err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(framePath, frame, 0600); err != nil {
		t.Fatal(err)
	}
	bundle := filepath.Join(tmp, "bundle")
	if err = os.Mkdir(bundle, 0700); err != nil {
		t.Fatal(err)
	}
	zeroToken, zeroDigest := "00000000000000000000000000000000", "0000000000000000000000000000000000000000000000000000000000000000"
	controlMessage := func(kind duplex.Kind, publication uint64, messageID, requestID, requestSHA, challenge string, payload []byte) []byte {
		t.Helper()
		h := duplex.Header{Kind: kind, Runtime: id.Runtime, Arena: id.Arena, Session: id.Session, Owner: id.Owner, ActorBinding: id.ActorBinding,
			RequestID: requestID, MessageID: messageID, Challenge: challenge, Fence: id.Fence, PublicationSeq: publication, TransportAttempt: 17,
			PublicationBegin: publication * 2, PublicationEnd: publication * 2, CreatedUTCMillis: 7654321, RequestSHA256: requestSHA}
		if requestID != zeroToken {
			h.RequestSeq = 9007199254740993
			h.TotalBytes = uint32(len("return true"))
			h.BudgetMillis = 120000
		}
		out, e := duplex.EncodeMessage(duplex.Message{Header: h, Payload: payload})
		if e != nil {
			t.Fatal(e)
		}
		return out
	}
	bindWire := controlMessage(duplex.Bind, 1, "70112233445566778899aabbccddeeff", zeroToken, zeroDigest, zeroToken, nil)
	closeWire := controlMessage(duplex.Close, 1, "71112233445566778899aabbccddeeff", "50112233445566778899aabbccddeeff", frames[0].Header.RequestSHA256, zeroToken, nil)
	cancelWire := controlMessage(duplex.Cancel, 1, "72112233445566778899aabbccddeeff", "50112233445566778899aabbccddeeff", frames[0].Header.RequestSHA256, zeroToken, nil)
	commitWire := controlMessage(duplex.Commit, 1, "73112233445566778899aabbccddeeff", "50112233445566778899aabbccddeeff", frames[0].Header.RequestSHA256, "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", nil)
	for name, wire := range map[string][]byte{"bind.bin": bindWire, "close.bin": closeWire, "cancel.bin": cancelWire, "commit.bin": commitWire} {
		if e := os.WriteFile(filepath.Join(bundle, name), wire, 0600); e != nil {
			t.Fatal(e)
		}
	}
	body := []byte(`{"ok":false,"error":"closed_before_execution","resourcesReleased":true}`)
	bodySHA := sha256.Sum256(body)
	bodyDigest := hex.EncodeToString(bodySHA[:])
	ackPayload, err := duplex.EncodeResultAck(duplex.ResultManifest{RequestID: "50112233445566778899aabbccddeeff", RequestSHA256: frames[0].Header.RequestSHA256, State: "cancelled", SHA256: bodyDigest, Bytes: uint32(len(body)), Pages: 1, PageSHA256: []string{bodyDigest}})
	if err != nil {
		t.Fatal(err)
	}
	ackWire := controlMessage(duplex.ResultAck, 1, "74112233445566778899aabbccddeeff", "50112233445566778899aabbccddeeff", frames[0].Header.RequestSHA256, zeroToken, ackPayload)
	if err = os.WriteFile(filepath.Join(bundle, "ack.bin"), ackWire, 0600); err != nil {
		t.Fatal(err)
	}
	repairHeader := duplex.Header{Kind: duplex.Repair, Runtime: id.Runtime, Arena: "90112233445566778899aabbccddeeff", Session: id.Session, Owner: id.Owner,
		ActorBinding: id.ActorBinding, RequestID: zeroToken, MessageID: "75112233445566778899aabbccddeeff", Challenge: "abababababababababababababababab",
		Fence: id.Fence, PublicationSeq: 2, TransportAttempt: 1, CreatedUTCMillis: 7654321, RequestSHA256: zeroDigest, PublicationBegin: 4, PublicationEnd: 4}
	repairWire, err := duplex.EncodeMessage(duplex.Message{Header: repairHeader})
	if err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(filepath.Join(bundle, "repair.bin"), repairWire, 0600); err != nil {
		t.Fatal(err)
	}
	// A separate owner must prove the exact closed/released old binding. The new
	// host restarts request sequence numbering at 1; old-session input is tested
	// against this new identity in the Lua harness.
	newReqID := "b0112233445566778899aabbccddeeff"
	firstRun, e := duplex.NewFrames(id, "a0112233445566778899aabbccddeeff", 500, 1, 7654321, 120000, []byte("return true"))
	if e != nil {
		t.Fatal(e)
	}
	put := func(name string, msg duplex.Message) {
		t.Helper()
		wire, e := duplex.EncodeMessage(msg)
		if e != nil {
			t.Fatal(e)
		}
		if e = os.WriteFile(filepath.Join(bundle, name), wire, 0600); e != nil {
			t.Fatal(e)
		}
	}
	put("fresh-frame.bin", firstRun[0])
	ctl := func(identity duplex.Identity, kind duplex.Kind, pub uint64, mid, rid, digest, challenge string, seq uint64, budget, total uint32, payload []byte) duplex.Message {
		h := duplex.Header{Kind: kind, Runtime: identity.Runtime, Arena: identity.Arena, Session: identity.Session, Owner: identity.Owner,
			ActorBinding: identity.ActorBinding, RequestID: rid, MessageID: mid, Challenge: challenge, Fence: identity.Fence,
			PublicationSeq: pub, TransportAttempt: 1, PublicationBegin: pub * 2, PublicationEnd: pub * 2, CreatedUTCMillis: 7654321,
			RequestSHA256: digest, RequestSeq: seq, BudgetMillis: budget, TotalBytes: total}
		return duplex.Message{Header: h, Payload: payload}
	}
	put("fresh-commit.bin", ctl(id, duplex.Commit, 1, "a1112233445566778899aabbccddeeff", firstRun[0].Header.RequestID, firstRun[0].Header.RequestSHA256, "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", 500, 120000, uint32(len("return true")), nil))
	successBody := []byte(`{"ok":true,"result":true,"resourcesReleased":true}`)
	successSum := sha256.Sum256(successBody)
	successDigest := hex.EncodeToString(successSum[:])
	successAck, e := duplex.EncodeResultAck(duplex.ResultManifest{RequestID: firstRun[0].Header.RequestID, RequestSHA256: firstRun[0].Header.RequestSHA256, State: "success", SHA256: successDigest, Bytes: uint32(len(successBody)), Pages: 1, PageSHA256: []string{successDigest}})
	if e != nil {
		t.Fatal(e)
	}
	put("fresh-ack.bin", ctl(id, duplex.ResultAck, 1, "a2112233445566778899aabbccddeeff", firstRun[0].Header.RequestID, firstRun[0].Header.RequestSHA256, zeroToken, 500, 0, 0, successAck))
	put("fresh-close.bin", ctl(id, duplex.Close, 1, "a3112233445566778899aabbccddeeff", zeroToken, zeroDigest, zeroToken, 0, 0, 0, nil))
	oldBox := duplex.Sendbox{Identity: id, ActorGUID: "Player-1-1", Character: "Paladin", Realm: "Realm", Build: "12.1.0.12345", Product: "retail", Release: "3.1.1",
		Schema: "lycheedev.mailbox.v1", LayoutID: "single-data-row-v1", Phase: "closed", ResourcesReleased: true, ActorReady: true, ControlReady: true,
		StatusSequence: 1, Heartbeat: 1,
		Released: &duplex.Released{RequestID: firstRun[0].Header.RequestID, RequestSHA256: firstRun[0].Header.RequestSHA256}}
	newIdentity, e := duplex.NextIdentity(oldBox, "c0112233445566778899aabbccddeeff", "d0112233445566778899aabbccddeeff")
	if e != nil {
		t.Fatal(e)
	}
	proof, e := duplex.EncodeClosedBindProof(oldBox)
	if e != nil {
		t.Fatal(e)
	}
	put("fresh-bind.bin", ctl(newIdentity, duplex.Bind, 1, "a4112233445566778899aabbccddeeff", zeroToken, zeroDigest, zeroToken, 0, 0, 0, proof))
	badFenceIdentity := newIdentity
	badFenceIdentity.Fence++
	put("bad-fence-bind.bin", ctl(badFenceIdentity, duplex.Bind, 2, "a8112233445566778899aabbccddeeff", zeroToken, zeroDigest, zeroToken, 0, 0, 0, nil))
	secondRun, e := duplex.NewFrames(newIdentity, newReqID, 1, 1, 7654321, 120000, []byte("return true"))
	if e != nil {
		t.Fatal(e)
	}
	put("second-frame.bin", secondRun[0])
	put("second-commit.bin", ctl(newIdentity, duplex.Commit, 1, "a5112233445566778899aabbccddeeff", newReqID, secondRun[0].Header.RequestSHA256, "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", 1, 120000, uint32(len("return true")), nil))
	secondAck, e := duplex.EncodeResultAck(duplex.ResultManifest{RequestID: newReqID, RequestSHA256: secondRun[0].Header.RequestSHA256, State: "success", SHA256: successDigest, Bytes: uint32(len(successBody)), Pages: 1, PageSHA256: []string{successDigest}})
	if e != nil {
		t.Fatal(e)
	}
	put("second-ack.bin", ctl(newIdentity, duplex.ResultAck, 1, "a6112233445566778899aabbccddeeff", newReqID, secondRun[0].Header.RequestSHA256, zeroToken, 1, 0, 0, secondAck))
	put("stale-frame.bin", firstRun[0])
	put("stale-control.bin", ctl(id, duplex.Cancel, 2, "a7112233445566778899aabbccddeeff", zeroToken, zeroDigest, zeroToken, 0, 0, 0, nil))
	control := duplex.Message{Header: duplex.Header{Kind: duplex.Reload, Runtime: id.Runtime, Arena: id.Arena, Session: id.Session, Owner: id.Owner, ActorBinding: id.ActorBinding, RequestID: "00000000000000000000000000000000", MessageID: "60112233445566778899aabbccddeeff", Challenge: "00000000000000000000000000000000", Fence: id.Fence, PublicationSeq: 9, TransportAttempt: 1, PublicationBegin: 18, PublicationEnd: 18, RequestSHA256: "0000000000000000000000000000000000000000000000000000000000000000"}}
	controlWire, err := duplex.EncodeMessage(control)
	if err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(controlPath, controlWire, 0600); err != nil {
		t.Fatal(err)
	}
	box := duplex.Sendbox{Identity: id, ActorGUID: "Player-1-1", Character: "Paladin", Realm: "Realm", Build: "12.1.0.12345", Product: "retail", Release: "3.1.1", ResourcesReleased: true, Schema: "lycheedev.mailbox.v1", LayoutID: "single-data-row-v1", Phase: "idle", ActorReady: true, Ready: true, TransportReady: true, BusinessReady: true, ControlReady: true, StatusSequence: 1, Heartbeat: 1, Receipts: map[string]duplex.Receipt{}}
	plain, err := json.Marshal(box)
	if err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(inputPath, plain, 0600); err != nil {
		t.Fatal(err)
	}
	output, err := exec.Command(lua, "duplex.lua", root, framePath, controlPath, inputPath, outputPath, bundle).CombinedOutput()
	if err != nil {
		t.Fatalf("%v\n%s", err, output)
	}
	if !bytes.Contains(output, []byte("Go wire, exact u64, ACK, repair, close and fresh-owner fencing passed")) {
		t.Fatalf("missing golden result: %s", output)
	}
	wire, err := os.ReadFile(outputPath)
	if err != nil {
		t.Fatal(err)
	}
	got, err := duplex.DecodeSendbox(wire)
	if err != nil {
		t.Fatal(err)
	}
	if got.Identity != box.Identity || got.Schema != box.Schema || got.StatusSequence != 1 || got.Heartbeat != 1 || got.Ready != got.BusinessReady {
		t.Fatalf("Lua sendbox envelope changed the Go contract: %+v", got)
	}
}

func TestDuplexLuaOneMiBAndRepeatedRelease(t *testing.T) {
	lua := luaRuntime(t)
	root, err := filepath.Abs("../../")
	if err != nil {
		t.Fatal(err)
	}
	tmp := t.TempDir()
	largePath, opsPath, bindPath := filepath.Join(tmp, "large.bin"), filepath.Join(tmp, "ops.bin"), filepath.Join(tmp, "bind.bin")
	id := duplex.Identity{Runtime: "00112233445566778899aabbccddeeff", Arena: "10112233445566778899aabbccddeeff", Session: "20112233445566778899aabbccddeeff", Owner: "30112233445566778899aabbccddeeff", ActorBinding: "40112233445566778899aabbccddeeff", Fence: 11}
	zeroID, zeroDigest := "00000000000000000000000000000000", "0000000000000000000000000000000000000000000000000000000000000000"
	control := func(kind duplex.Kind, seq uint64, req duplex.Header, messageID, challenge string, payload []byte) []byte {
		t.Helper()
		h := duplex.Header{Kind: kind, Runtime: id.Runtime, Arena: id.Arena, Session: id.Session, Owner: id.Owner, ActorBinding: id.ActorBinding,
			RequestID: zeroID, MessageID: messageID, Challenge: challenge, Fence: id.Fence, PublicationSeq: seq, PublicationBegin: seq * 2, PublicationEnd: seq * 2,
			TransportAttempt: 1, CreatedUTCMillis: 7654321, RequestSHA256: zeroDigest}
		if req.RequestID != "" {
			h.RequestID = req.RequestID
			h.RequestSeq = req.RequestSeq
			h.RequestSHA256 = req.RequestSHA256
			h.TransportAttempt = req.TransportAttempt
			h.CreatedUTCMillis = req.CreatedUTCMillis
			h.BudgetMillis = req.BudgetMillis
			h.TotalBytes = req.TotalBytes
		}
		wire, e := duplex.EncodeMessage(duplex.Message{Header: h, Payload: payload})
		if e != nil {
			t.Fatal(e)
		}
		return wire
	}
	bind := control(duplex.Bind, 1, duplex.Header{}, "70112233445566778899aabbccddeeff", zeroID, nil)
	if err = os.WriteFile(bindPath, bind, 0600); err != nil {
		t.Fatal(err)
	}
	largeSource := []byte("return true" + strings.Repeat(" ", duplex.MaxSourceBytes-len("return true")))
	largeFrames, err := duplex.NewFrames(id, "50112233445566778899aabbccddeeff", 9007199254740993, 17, 7654321, 120000, largeSource)
	if err != nil {
		t.Fatal(err)
	}
	var large bytes.Buffer
	for _, m := range largeFrames {
		wire, e := duplex.EncodeMessage(m)
		if e != nil {
			t.Fatal(e)
		}
		large.Write(wire)
	}
	if err = os.WriteFile(largePath, large.Bytes(), 0600); err != nil {
		t.Fatal(err)
	}
	resultBody := []byte(`{"ok":true,"result":true,"resourcesReleased":true}`)
	sum := sha256.Sum256(resultBody)
	resultDigest := hex.EncodeToString(sum[:])
	manifest := duplex.ResultManifest{State: "success", SHA256: resultDigest, Bytes: uint32(len(resultBody)), Pages: 1, PageSHA256: []string{resultDigest}}
	var ops bytes.Buffer
	appendRequest := func(requestID string, seq uint64, attempt uint64, source []byte, pub uint64, includeFrame bool) {
		t.Helper()
		frames, e := duplex.NewFrames(id, requestID, seq, attempt, 7654321, 120000, source)
		if e != nil {
			t.Fatal(e)
		}
		if includeFrame {
			fw, e := duplex.EncodeMessage(frames[0])
			if e != nil {
				t.Fatal(e)
			}
			ops.Write(fw)
		}
		h := frames[0].Header
		cw := control(duplex.Commit, pub, h, fmt.Sprintf("%032x", 1000+pub), "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", nil)
		ops.Write(cw)
		manifest.RequestID = h.RequestID
		manifest.RequestSHA256 = h.RequestSHA256
		ack, e := duplex.EncodeResultAck(manifest)
		if e != nil {
			t.Fatal(e)
		}
		aw := control(duplex.ResultAck, pub, h, fmt.Sprintf("%032x", 2000+pub), zeroID, ack)
		ops.Write(aw)
	}
	appendRequest("50112233445566778899aabbccddeeff", 9007199254740993, 17, largeSource, 1, false)
	for i := 1; i <= 140; i++ {
		appendRequest(fmt.Sprintf("%032x", 3000+i), 9007199254740993+uint64(i), 1, []byte("return 1"), uint64(i+1), true)
	}
	if err = os.WriteFile(opsPath, ops.Bytes(), 0600); err != nil {
		t.Fatal(err)
	}
	output, err := exec.Command(lua, "duplex_stress.lua", root, largePath, opsPath, bindPath).CombinedOutput()
	if err != nil {
		t.Fatalf("%v\n%s", err, output)
	}
	if !bytes.Contains(output, []byte("1 MiB transfer and 140 released requests retained bounded memory")) {
		t.Fatalf("missing stress result: %s", output)
	}
	t.Log(strings.TrimSpace(string(output)))
}
