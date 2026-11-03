package publish

import (
	"context"
	"crypto/md5"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"net/http"
	"net/url"
	"strconv"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"github.com/gorilla/websocket"
)

const ddCCEndpoint = "wss://teamlink-ws.cc.163.com/"
const ddCCOrigin = "https://api.cc.163.com"

type ddCCDevice struct{ clientNo, mac, osVersion string }
type ddCC struct {
	socket                   *websocket.Conn
	mu                       sync.Mutex
	requestMu                sync.Mutex
	pending                  map[string]chan ddCCResponse
	done                     chan struct{}
	stop                     sync.Once
	sequence                 atomic.Uint64
	prefix, cookie, uid, eid string
}

func ddCCLost() error {
	return capability("publish.dd_cc_lost", "DD task connection ended; the original session cannot be replayed")
}
func ddRandom() string {
	var b [24]byte
	if _, err := rand.Read(b[:]); err != nil {
		panic(err)
	}
	return hex.EncodeToString(b[:])
}
func ddOpenCC(ctx context.Context, credential ddCredential, appID string, device ddCCDevice) (*ddCC, error) {
	return ddOpenCCAt(ctx, credential, appID, device, ddCCEndpoint, 30*time.Second)
}
func ddOpenCCAt(ctx context.Context, credential ddCredential, appID string, device ddCCDevice, endpoint string, heartbeat time.Duration) (*ddCC, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if appID == "" || device.clientNo == "" || device.mac == "" {
		return nil, ddCredentialError()
	}
	dialer := websocket.Dialer{HandshakeTimeout: 15 * time.Second, Proxy: http.ProxyFromEnvironment, EnableCompression: false}
	socket, _, err := dialer.DialContext(ctx, endpoint, nil)
	if err != nil {
		return nil, ddCCLost()
	}
	socket.SetReadLimit(ddCCLimit)
	c := &ddCC{socket: socket, pending: map[string]chan ddCCResponse{}, done: make(chan struct{}), prefix: ddRandom()}
	go c.read()
	failed := true
	defer func() {
		if failed {
			c.close()
		}
	}()
	if _, err = c.request(ctx, 1, map[string]any{"mac": device.mac, "clientNo": device.clientNo, "clientType": 412, "version": "1.1.30", "versionCode": "100130"}); err != nil {
		return nil, err
	}
	info, err := c.request(ctx, 3, map[string]any{"loginType": credential.loginType(), "ccToken": credential.token, "mobileId": appID, "session": c.prefix + ":0", "urs": credential.username(), "appVer": "1.1.30", "osName": "windows", "osVer": device.osVersion, "macAddr": device.mac, "biDesc": map[string]any{}})
	if err != nil {
		return nil, err
	}
	cookie, _ := info["cookieValue"].(string)
	if info["cookieName"] != "NTES_YD_SESS" || cookie == "" || len(cookie) > 65536 || strings.ContainsAny(cookie, ";\r\n\x00") {
		return nil, ddCCProtocol()
	}
	c.cookie = cookie
	if c.uid, err = ddIdentityText(info["uid"]); err != nil {
		return nil, ddCCProtocol()
	}
	if c.eid, err = ddIdentityText(info["eid"]); err != nil {
		return nil, ddCCProtocol()
	}
	failed = false
	go c.heartbeat(heartbeat)
	return c, nil
}
func (c *ddCC) active() error {
	select {
	case <-c.done:
		return ddCCLost()
	default:
		return nil
	}
}
func (c *ddCC) close() error { c.stop.Do(func() { close(c.done); c.socket.Close() }); return nil }
func (c *ddCC) read() {
	defer c.close()
	for {
		kind, raw, err := c.socket.ReadMessage()
		if err != nil || kind != websocket.BinaryMessage {
			return
		}
		response, err := ddCCDecode(raw)
		if err != nil {
			return
		}
		if response.sid == ddCCSID && response.command == 5 {
			return
		}
		c.mu.Lock()
		reply := c.pending[response.sequence]
		c.mu.Unlock()
		if reply != nil {
			select {
			case reply <- response:
			default:
			}
		}
	}
}
func (c *ddCC) request(ctx context.Context, command uint16, body any) (map[string]any, error) {
	c.requestMu.Lock()
	defer c.requestMu.Unlock()
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if err := c.active(); err != nil {
		return nil, err
	}
	ctx, cancel := context.WithTimeout(ctx, 15*time.Second)
	defer cancel()
	seq := c.prefix + strconv.FormatUint(c.sequence.Add(1), 10)
	raw, err := ddCCEncode(command, seq, body)
	if err != nil {
		return nil, err
	}
	reply := make(chan ddCCResponse, 1)
	c.mu.Lock()
	c.pending[seq] = reply
	c.mu.Unlock()
	defer func() { c.mu.Lock(); delete(c.pending, seq); c.mu.Unlock() }()
	deadline, _ := ctx.Deadline()
	c.socket.SetWriteDeadline(deadline)
	if c.socket.WriteMessage(websocket.BinaryMessage, raw) != nil {
		c.close()
		return nil, ddCCLost()
	}
	for {
		select {
		case <-ctx.Done():
			c.close()
			return nil, ctx.Err()
		case <-c.done:
			return nil, ddCCLost()
		case response := <-reply:
			if response.sid != ddCCSID || response.command != command {
				continue
			}
			if response.code != 0 {
				return nil, &Error{Code: "publish.dd_cc_rejected", Message: "CC rejected the session request", Exit: 5, Details: map[string]any{"command": command, "platformCode": response.code}}
			}
			return response.data, nil
		}
	}
}
func (c *ddCC) heartbeat(interval time.Duration) {
	ticker := time.NewTicker(interval)
	defer ticker.Stop()
	for {
		select {
		case <-c.done:
			return
		case <-ticker.C:
			if _, err := c.request(context.Background(), 4, map[string]any{}); err != nil {
				c.close()
				return
			}
		}
	}
}
func (c *ddCC) jwt(ctx context.Context, h *HTTP) (string, error) {
	data, err := c.request(ctx, 8, map[string]any{})
	if err != nil {
		return "", err
	}
	token, _ := data["token"].(string)
	ts := ddInteger(data["nowTime"]) / 1000
	if token == "" || len(token) > 65536 || ts <= 0 {
		return "", ddCCProtocol()
	}
	for attempt := 0; attempt < 2; attempt++ {
		if err = c.active(); err != nil {
			return "", err
		}
		sum := md5.Sum([]byte(fmt.Sprintf("webcc_%d_%s", ts, token)))
		headers := ddHeaders()
		headers.Set("authentication", hex.EncodeToString(sum[:]))
		q := url.Values{"appid": {"webcc"}, "uid": {c.uid}, "eid": {c.eid}, "timestamp": {strconv.FormatInt(ts, 10)}}
		value, e := h.JSON(ctx, http.MethodGet, Endpoint(ddCCOrigin, "/v1/jwt", q), headers, nil)
		if e != nil {
			return "", e
		}
		if value["code"] == "OK" {
			v, _ := value["data"].(map[string]any)
			jwt, _ := v["jwt"].(string)
			if strings.TrimSpace(jwt) == "" || len(jwt) > 65536 || strings.ContainsAny(jwt, "\r\n\x00") {
				return "", ddCCProtocol()
			}
			return jwt, nil
		}
		if attempt == 0 && value["code"] == "INVALID_TIMESTAMP" {
			ts = ddInteger(value["time"])
			if ts > 0 {
				continue
			}
		}
		return "", failure("publish.dd_jwt_rejected", "CC JWT exchange was rejected")
	}
	return "", ddCCProtocol()
}
func (c *ddCC) channels(ctx context.Context) (any, error) {
	h := NewHTTP()
	jwt, err := c.jwt(ctx, h)
	if err != nil {
		return nil, err
	}
	headers := ddHeaders()
	headers.Set("authentication", jwt)
	value, err := h.JSON(ctx, http.MethodGet, Endpoint(ddCCOrigin, "/v1/mixteammsgproxy/channelList", url.Values{"source": {"pluginPublish"}}), headers, nil)
	if err != nil {
		return nil, err
	}
	if !ddCCAccepted(value) {
		return nil, failure("publish.dd_channels", "CC channel query was rejected")
	}
	return ddProjectChannels(value, jwt)
}
func ddCCAccepted(value map[string]any) bool {
	if value["code"] == "OK" {
		return true
	}
	switch code := value["code"].(type) {
	case json.Number:
		return code == "0" || code == "200"
	case string:
		return code == "0" || code == "200"
	}
	return false
}
func ddProjectChannels(value map[string]any, secrets ...string) (any, error) {
	if !ddCCAccepted(value) {
		return nil, failure("publish.dd_channels", "CC channel query was rejected")
	}
	var rows []any
	switch data := value["data"].(type) {
	case []any:
		rows = data
	case map[string]any:
		for _, key := range []string{"list", "items", "channels", "data"} {
			if v, ok := data[key]; ok {
				items, ok := v.([]any)
				if !ok {
					return nil, ddCCProtocol()
				}
				rows = items
				if len(items) > 0 {
					break
				}
			}
		}
	default:
		return nil, ddCCProtocol()
	}
	if rows == nil {
		return nil, ddCCProtocol()
	}
	out := []map[string]any{}
	seen := map[[3]string]bool{}
	var walk func([]any, string, string, int) error
	walk = func(rows []any, roomID, roomName string, depth int) error {
		if depth >= 64 {
			return ddCCProtocol()
		}
		for _, raw := range rows {
			item, ok := raw.(map[string]any)
			if !ok {
				return ddCCProtocol()
			}
			var bad error
			text := func(keys ...string) string {
				for _, key := range keys {
					if value, ok := item[key]; ok && value != nil {
						if value == "" {
							continue
						}
						v, e := ddIdentityText(value)
						if e != nil {
							bad = ddCCProtocol()
							return ""
						}
						return v
					}
				}
				return ""
			}
			explicit := text("teamId", "team_id", "room_id")
			room := roomID
			if explicit != "" {
				room = explicit
			}
			name := text("teamName", "team_name", "room_name")
			if name == "" {
				name = roomName
			}
			id := text("channelId", "channel_id")
			typ := text("channelType", "channel_type")
			key := [3]string{room, id, typ}
			if room != "" && (explicit != "" || id != "") && !seen[key] {
				seen[key] = true
				channelName := text("channelName", "channel_name")
				if channelName == "" && id != "" {
					channelName = text("name")
				}
				out = append(out, map[string]any{"room_id": room, "room_name": name, "channel_id": id, "channel_name": channelName, "channel_type": typ})
				if len(out) > 10000 {
					return ddCCProtocol()
				}
			}
			if bad != nil {
				return bad
			}
			for _, key := range []string{"channelList", "channels", "children", "items"} {
				if v, ok := item[key]; ok && v != nil {
					children, ok := v.([]any)
					if !ok {
						return ddCCProtocol()
					}
					if err := walk(children, room, name, depth+1); err != nil {
						return err
					}
				}
			}
		}
		return nil
	}
	if err := walk(rows, "", "", 0); err != nil {
		return nil, err
	}
	for _, item := range out {
		for _, v := range item {
			if s, ok := v.(string); ok {
				for _, secret := range secrets {
					if secret != "" && strings.Contains(s, secret) {
						return nil, ddCCProtocol()
					}
				}
			}
		}
	}
	return map[string]any{"total": len(out), "items": out, "dependencies": []any{map[string]any{"parent": "room_id", "children": []string{"channel_id", "channel_type"}}}}, nil
}
