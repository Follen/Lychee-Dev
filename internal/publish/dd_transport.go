package publish

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/url"
	"strconv"
	"strings"
	"time"
)

const ddOrigin = "https://uiapi.w.163.com"

type ddTransport struct {
	http          *HTTP
	signer        ddSigner
	cookie, token string
	origin        string
	timestamp     int64
	sampled       time.Time
}

func (t *ddTransport) close() error { return t.signer.Close() }
func (t *ddTransport) clock(ctx context.Context) error {
	value, err := t.http.JSON(ctx, http.MethodGet, t.origin+"/server/ts", ddHeaders(), nil)
	if err != nil {
		return err
	}
	code, _ := positiveOrZero(value["code"])
	result, _ := value["result"].(map[string]any)
	ts := ddInteger(result["ts"])
	if code != 200 || ts <= 0 {
		return failure("publish.dd_clock", "DD server did not return a valid timestamp")
	}
	t.timestamp, t.sampled = ts, time.Now()
	return nil
}
func ddInteger(value any) int64 {
	switch n := value.(type) {
	case json.Number:
		i, _ := n.Int64()
		return i
	case string:
		i, _ := strconv.ParseInt(n, 10, 64)
		return i
	}
	return 0
}
func ddHeaders() http.Header {
	return http.Header{"User-Agent": {"Mozilla/5.0 (Windows NT 10.0; WOW64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/108.0.0.0 Safari/537.36 app/df_client dfVersion/100130"}, "Origin": {"https://dev.cc.163.com"}, "Referer": {"https://dev.cc.163.com/"}}
}
func (t *ddTransport) call(ctx context.Context, path string, query url.Values, body any, mutation bool) (map[string]any, error) {
	if t.cookie == "" || strings.ContainsAny(t.cookie, ";\r\n\x00") || strings.ContainsAny(t.token, "\r\n\x00") {
		return nil, capability("publish.dd_session", "invalid or absent DD session")
	}
	if !strings.HasPrefix(path, "/") || strings.HasPrefix(path, "//") || strings.ContainsAny(path, "?#\\%") || strings.Contains(path, "/../") {
		return nil, invalid("$route", "invalid DD route")
	}
	if t.timestamp == 0 {
		if err := t.clock(ctx); err != nil {
			return nil, err
		}
	}
	original := Endpoint(t.origin, path, query)
	var raw []byte
	var err error
	if body != nil {
		raw, err = json.Marshal(body)
		if err != nil {
			return nil, err
		}
	}
	timestamp := strconv.FormatInt(t.timestamp+int64(time.Since(t.sampled)/time.Second), 10)
	signed, err := t.signer.Sign(ctx, original, raw, [][2]string{{"x-w163-token", t.token}, {"x-timestamp", timestamp}})
	if err != nil {
		return nil, err
	}
	if err = ddSignedTarget(original, signed); err != nil {
		return nil, err
	}
	headers := ddHeaders()
	headers.Set("Cookie", "NTES_YD_SESS="+t.cookie)
	headers.Set("x-w163-token", t.token)
	headers.Set("x-timestamp", timestamp)
	method := http.MethodGet
	var reader *bytes.Reader
	if body != nil {
		method = http.MethodPost
		reader = bytes.NewReader(raw)
	}
	var response any
	if reader == nil {
		response, err = t.http.request(ctx, method, signed, headers, nil, "application/json;charset=UTF-8")
	} else {
		response, err = t.http.request(ctx, method, signed, headers, reader, "application/json;charset=UTF-8")
	}
	if err != nil {
		var platformError *Error
		if errors.As(err, &platformError) && platformError.Code == "publish.http_status" {
			details, _ := platformError.Details.(map[string]any)
			status, _ := details["httpStatus"].(int)
			if status == http.StatusUnauthorized || status == http.StatusForbidden {
				rejected := *platformError
				rejected.Code = "publish.dd_rejected"
				rejected.Message = "DD refused authorization for the selected operation"
				return nil, &rejected
			}
		}
		return nil, err
	}
	result, ok := response.(map[string]any)
	if !ok {
		return nil, failure("publish.dd_data", "DD response is not an object")
	}
	code, parseErr := strconv.ParseInt(ddText(result["code"]), 10, 64)
	if parseErr != nil {
		return nil, failure("publish.dd_data", "DD response omitted a valid status code")
	}
	if code != 200 {
		return nil, &Error{Code: "publish.dd_rejected", Message: "DD rejected the selected operation", Exit: 5, Details: map[string]any{"platformCode": code, "mutation": mutation}}
	}
	if _, ok = result["result"]; !ok {
		return nil, failure("publish.dd_data", "DD response omitted result")
	}
	return result, nil
}
func ddSignedTarget(original, signed string) error {
	a, err := url.Parse(original)
	if err != nil {
		return invalid("$url", "invalid original route")
	}
	b, err := url.Parse(signed)
	if err != nil || a.Scheme != b.Scheme || a.Host != b.Host || a.EscapedPath() != b.EscapedPath() || b.User != nil || b.Fragment != "" {
		return capability("publish.dd_signed_target", "signature changed selected route")
	}
	for key, want := range a.Query() {
		got := b.Query()[key]
		if len(got) != len(want) {
			return capability("publish.dd_signed_target", "signature changed selected arguments")
		}
		counts := map[string]int{}
		for _, v := range want {
			counts[v]++
		}
		for _, v := range got {
			counts[v]--
		}
		for _, n := range counts {
			if n != 0 {
				return capability("publish.dd_signed_target", "signature changed selected arguments")
			}
		}
	}
	return nil
}
