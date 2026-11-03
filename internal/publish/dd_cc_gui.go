package publish

import (
	"context"
	"crypto/md5"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"math"
	"net/http"
	"net/url"
	"strconv"
	"strings"
	"time"
)

func ddGUIJWTValid(token string, s ddManagedSession, now float64) (bool, error) {
	if token == "" || len(token) > 16384 || math.IsNaN(now) || math.IsInf(now, 0) || now <= 0 {
		return false, ddManagedError()
	}
	parts := strings.Split(token, ".")
	if len(parts) != 3 {
		return false, ddManagedError()
	}
	for _, part := range parts {
		if part == "" {
			return false, ddManagedError()
		}
		for _, c := range part {
			if !(c >= 'a' && c <= 'z' || c >= 'A' && c <= 'Z' || c >= '0' && c <= '9' || c == '_' || c == '-') {
				return false, ddManagedError()
			}
		}
	}
	parse := func(part string) (map[string]any, error) {
		raw, err := base64.RawURLEncoding.Strict().DecodeString(part)
		if err != nil {
			return nil, ddManagedError()
		}
		var out map[string]any
		if ddJSON(raw, &out) != nil {
			return nil, ddManagedError()
		}
		return out, nil
	}
	header, err := parse(parts[0])
	if err != nil {
		return false, err
	}
	claims, err := parse(parts[1])
	if err != nil {
		return false, err
	}
	alg, _ := header["alg"].(string)
	if alg == "" || strings.EqualFold(alg, "none") || claims["appid"] != "webcc" {
		return false, ddManagedError()
	}
	var user map[string]any
	switch v := claims["user"].(type) {
	case string:
		if ddJSON([]byte(v), &user) != nil {
			return false, ddManagedError()
		}
	case map[string]any:
		user = v
	default:
		return false, ddManagedError()
	}
	uid, e := ddIdentityText(user["uid"])
	if e != nil || uid != s.uid {
		return false, ddManagedError()
	}
	eid, e := ddIdentityText(user["eid"])
	if e != nil || eid != s.eid {
		return false, ddManagedError()
	}
	n, ok := claims["exp"].(json.Number)
	if !ok {
		return false, ddManagedError()
	}
	expiry, e := n.Float64()
	if e != nil || math.IsInf(expiry, 0) || math.IsNaN(expiry) || expiry < 0 || expiry > now+1200 {
		return false, ddManagedError()
	}
	return expiry > now+5, nil
}
func ddGUIJWT(ctx context.Context, s ddManagedSession, h *HTTP, origin string, now float64) (string, error) {
	if s.cached != "" {
		valid, err := ddGUIJWTValid(s.cached, s, now)
		if err != nil {
			return "", err
		}
		if valid {
			return s.cached, nil
		}
	}
	ts := int64(now + s.delta)
	if ts <= 0 {
		return "", ddManagedError()
	}
	sum := md5.Sum([]byte("webcc_" + strconv.FormatInt(ts, 10) + "_" + s.loginToken))
	headers := ddHeaders()
	headers.Set("authentication", hex.EncodeToString(sum[:]))
	value, err := h.JSON(ctx, http.MethodGet, Endpoint(origin, "/v1/jwt", url.Values{"appid": {"webcc"}, "uid": {s.uid}, "eid": {s.eid}, "timestamp": {strconv.FormatInt(ts, 10)}}), headers, nil)
	if err != nil {
		return "", err
	}
	data, _ := value["data"].(map[string]any)
	token, _ := data["jwt"].(string)
	if !ddCCAccepted(value) {
		return "", failure("publish.dd_gui_jwt", "DD rejected the existing GUI token exchange")
	}
	valid, err := ddGUIJWTValid(token, s, now)
	if err != nil || !valid {
		return "", ddManagedError()
	}
	return token, nil
}
func ddGUIChannelsWith(ctx context.Context, reader *ddManagedReader, base uint64, ccid string, h *HTTP, origin string, verify func(context.Context) error) (any, error) {
	if err := verify(ctx); err != nil {
		return nil, err
	}
	s, err := reader.snapshot(base)
	if err != nil {
		return nil, err
	}
	if s.ccid != ccid {
		return nil, ddManagedError()
	}
	if err = verify(ctx); err != nil {
		return nil, err
	}
	if err = reader.verify(s); err != nil {
		return nil, err
	}
	now := func() float64 { return float64(time.Now().UnixMilli()) / 1000 }
	token, err := ddGUIJWT(ctx, s, h, origin, now())
	if err != nil {
		return nil, err
	}
	if err = reader.verify(s); err != nil {
		return nil, err
	}
	if err = verify(ctx); err != nil {
		return nil, err
	}
	valid, err := ddGUIJWTValid(token, s, now())
	if err != nil || !valid {
		return nil, ddManagedError()
	}
	headers := ddHeaders()
	headers.Set("authentication", token)
	value, err := h.JSON(ctx, http.MethodGet, Endpoint(origin, "/v1/mixteammsgproxy/channelList", url.Values{"source": {"pluginPublish"}}), headers, nil)
	if err != nil {
		return nil, err
	}
	if err = reader.verify(s); err != nil {
		return nil, err
	}
	if err = verify(ctx); err != nil {
		return nil, err
	}
	return ddProjectChannels(value, token, s.cached, s.loginToken)
}
