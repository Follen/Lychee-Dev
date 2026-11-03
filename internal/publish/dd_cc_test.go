package publish

import (
	"bytes"
	"compress/zlib"
	"context"
	"encoding/binary"
	"encoding/json"
	"github.com/gorilla/websocket"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync/atomic"
	"testing"
	"time"
)

func ddCCReplyForTest(command uint16, seq string, data any, compressed bool) []byte {
	ext, _ := json.Marshal(map[string]any{"seq": seq})
	prop, _ := json.Marshal(map[string]any{"extInfo": string(ext)})
	body, _ := json.Marshal(data)
	raw := make([]byte, 16)
	binary.LittleEndian.PutUint16(raw, ddCCSID)
	binary.LittleEndian.PutUint16(raw[2:], command)
	tail := binary.LittleEndian.AppendUint16(nil, uint16(len(prop)))
	tail = append(tail, prop...)
	tail = binary.LittleEndian.AppendUint32(tail, 0)
	tail = binary.LittleEndian.AppendUint16(tail, 0)
	tail = binary.LittleEndian.AppendUint16(tail, uint16(len(body)))
	tail = append(tail, body...)
	if compressed {
		binary.LittleEndian.PutUint32(raw[12:], 0x20000)
		raw = append(raw, 0, 0, 0, 0)
		var b bytes.Buffer
		w := zlib.NewWriter(&b)
		w.Write(tail)
		w.Close()
		return append(raw, b.Bytes()...)
	}
	return append(raw, tail...)
}
func TestDDCCPacketCompressionAndStrictCorrelation(t *testing.T) {
	for _, compressed := range []bool{false, true} {
		raw := ddCCReplyForTest(8, "original", map[string]any{"token": "synthetic-token", "nowTime": uint64(2000000000000)}, compressed)
		r, err := ddCCDecode(raw)
		if err != nil || r.sequence != "original" || r.command != 8 || r.data["token"] != "synthetic-token" {
			t.Fatalf("response: %#v %v", r, err)
		}
		if _, err = ddCCDecode(raw[:len(raw)-1]); err == nil {
			t.Fatal("accepted truncated packet")
		}
	}
	if _, err := ddCCDecode([]byte("short")); err == nil {
		t.Fatal("accepted short packet")
	}
	if _, err := ddCCEncode(1, "s", strings.Repeat("x", 65536)); err == nil {
		t.Fatal("accepted oversized request")
	}
}
func TestDDCCSingleLoginHeartbeatTokenAndLostSession(t *testing.T) {
	var logins, heartbeats, connections atomic.Int32
	kick := make(chan struct{})
	upgrader := websocket.Upgrader{}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		connections.Add(1)
		conn, err := upgrader.Upgrade(w, r, nil)
		if err != nil {
			return
		}
		defer conn.Close()
		for {
			_, raw, err := conn.ReadMessage()
			if err != nil {
				return
			}
			if len(raw) < 20 {
				return
			}
			cid := binary.LittleEndian.Uint16(raw[2:])
			n := int(binary.LittleEndian.Uint16(raw[16:]))
			if 18+n+2 > len(raw) {
				return
			}
			var prop map[string]string
			if json.Unmarshal(raw[18:18+n], &prop) != nil {
				return
			}
			var ext map[string]string
			json.Unmarshal([]byte(prop["extInfo"]), &ext)
			var data map[string]any
			json.Unmarshal(raw[20+n:], &data)
			response := map[string]any{}
			switch cid {
			case 1:
				if data["clientType"] != float64(412) {
					t.Error("wrong CC identity")
				}
			case 3:
				logins.Add(1)
				if data["loginType"] != "ursPhoneTokenNew" || data["urs"] != "123@mobile.163.com" {
					t.Error("wrong selected credential")
				}
				response = map[string]any{"cookieName": "NTES_YD_SESS", "cookieValue": "synthetic-cookie", "uid": "123", "eid": "456"}
			case 4:
				heartbeats.Add(1)
				select {
				case <-kick:
					conn.WriteMessage(websocket.BinaryMessage, ddCCReplyForTest(5, "", map[string]any{}, false))
					return
				default:
				}
			case 8:
				response = map[string]any{"token": "synthetic-live-token", "nowTime": 2000000000000}
			}
			if cid == 8 {
				conn.WriteMessage(websocket.BinaryMessage, ddCCReplyForTest(cid, "other-sequence", map[string]any{"token": "wrong"}, false))
			}
			if conn.WriteMessage(websocket.BinaryMessage, ddCCReplyForTest(cid, ext["seq"], response, cid == 3)) != nil {
				return
			}
		}
	}))
	defer server.Close()
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	c, err := ddOpenCCAt(ctx, ddCredential{method: 2, account: "123", token: "synthetic-cred"}, "synthetic-app", ddCCDevice{"12345678901234567890123456789012", "00-11-22-33-44-55", "10.0.1"}, "ws"+strings.TrimPrefix(server.URL, "http"), 20*time.Millisecond)
	if err != nil {
		t.Fatal(err)
	}
	defer c.close()
	if c.cookie != "synthetic-cookie" || c.uid != "123" {
		t.Fatal("identity")
	}
	value, err := c.request(ctx, 8, map[string]any{})
	if err != nil || value["token"] != "synthetic-live-token" {
		t.Fatalf("correlation %v %v", value, err)
	}
	close(kick)
	select {
	case <-c.done:
	case <-ctx.Done():
		t.Fatal("kick did not retire session")
	}
	if _, err = c.request(ctx, 8, map[string]any{}); err == nil {
		t.Fatal("lost session allowed token request")
	}
	if logins.Load() != 1 || connections.Load() != 1 || heartbeats.Load() < 1 {
		t.Fatal("login or connection replay")
	}
}
func TestDDCCCancelledBeforeDial(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := ddOpenCCAt(ctx, ddCredential{}, "app", ddCCDevice{}, "ws://127.0.0.1:1", time.Second); err != context.Canceled {
		t.Fatalf("%v", err)
	}
}
func TestDDChannelProjectionRejectsWrongEnvelopeAndPreservesRoomChoices(t *testing.T) {
	var v map[string]any
	ddJSON([]byte(`{"code":"OK","data":[{"teamId":12,"teamName":"房间","channelList":[{"channelId":34,"channelType":1,"channelName":"频道"}]}]}`), &v)
	out, err := ddProjectChannels(v, "synthetic-secret")
	if err != nil {
		t.Fatal(err)
	}
	m := out.(map[string]any)
	if m["total"] != 2 {
		t.Fatalf("%v", m)
	}
	v["code"] = "AUTH_FAILED"
	if _, err = ddProjectChannels(v); err == nil {
		t.Fatal("accepted failed envelope")
	}
	delete(v, "code")
	if _, err = ddProjectChannels(v); err == nil {
		t.Fatal("accepted missing envelope")
	}
	v["code"] = "OK"
	if _, err = ddProjectChannels(v, "房间"); err == nil {
		t.Fatal("echoed secret")
	}
}
