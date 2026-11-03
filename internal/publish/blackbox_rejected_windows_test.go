//go:build windows

package publish

import (
	"context"
	"net/http"
	"testing"
)

func TestBlackboxRejectedRenameCannotVerifyExistingValue(t *testing.T) {
	c := bbFixtureClient(t)
	writes := 0
	c.http.Transport = bbTestTransport(func(r *http.Request) (*http.Response, error) {
		switch r.URL.Path {
		case "/account/restore_login":
			return bbRestoreReply(), nil
		case "/wow/get_user_configuration_backup_list/":
			return bbReply(map[string]any{"backupList": []any{map[string]any{"ID": "one", "Name": "same"}}}), nil
		case "/wow/rename_user_configuration_backup/":
			writes++
			return bbTestResponse(200, `{"status":"failed","msg":"denied"}`), nil
		}
		t.Fatalf("unexpected route %s", r.URL.Path)
		return nil, nil
	})
	req := Request{RequestID: "rejected-rename", Input: []byte(`{"backup_id":"one","name":"same"}`)}
	first, err := c.run(context.Background(), "backup rename", req)
	requireRejectedExit(t, first, err)
	for _, action := range []string{"operation verify", "operation resume"} {
		result, err := c.run(context.Background(), action, Request{Input: []byte(`{"request":"rejected-rename"}`)})
		if err == nil || result.Complete || writes != 1 {
			t.Fatalf("%s: complete=%t writes=%d err=%v", action, result.Complete, writes, err)
		}
		requireRejectedExit(t, result, err)
	}
	result, err := c.run(context.Background(), "backup rename", req)
	requireRejectedExit(t, result, err)
	status, err := c.run(context.Background(), "operation status", Request{Input: []byte(`{"request":"rejected-rename"}`)})
	if err != nil || status.Submission != "rejected" || writes != 1 {
		t.Fatalf("status: %+v %v, writes=%d", status, err, writes)
	}
}

func TestBlackboxWriteResponseDistinguishesRejectionFromUnknown(t *testing.T) {
	for _, tc := range []struct {
		name, body string
		status     int
		rejected   bool
	}{
		{"rejected", `{"status":"failed"}`, 200, true},
		{"unauthorized", `{}`, 401, true},
		{"forbidden", `{}`, 403, true},
		{"missing-status", `{}`, 200, false},
		{"invalid-status", `{"status":true}`, 200, false},
		{"server-error", `{}`, 503, false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			c := bbFixtureClient(t)
			writes := 0
			c.http.Transport = bbTestTransport(func(r *http.Request) (*http.Response, error) {
				switch r.URL.Path {
				case "/account/restore_login":
					return bbRestoreReply(), nil
				case "/wow/open_platform/module/detail/":
					return bbReply(map[string]any{"module": map[string]any{"id": 7, "name": "same", "logoUrl": "https://example.test/a.png", "categoryIds": []int{2}, "type": 1, "desc": "", "official": "", "officialUrl": "", "coreFolders": "Example"}}), nil
				case "/wow/open_platform/module/update/":
					writes++
					return bbTestResponse(tc.status, tc.body), nil
				}
				t.Fatalf("unexpected route %s", r.URL.Path)
				return nil, nil
			})
			result, err := c.run(context.Background(), "plugin edit", Request{RequestID: "response", Input: []byte(`{"id":7,"name":"same"}`)})
			if err == nil || result.Complete {
				t.Fatal("write response error hidden")
			}
			if tc.rejected {
				requireRejectedExit(t, result, err)
			}
			result, err = c.run(context.Background(), "operation verify", Request{Input: []byte(`{"request":"response"}`)})
			if result.Complete == tc.rejected || (err == nil) == tc.rejected || writes != 1 {
				t.Fatalf("verify: %+v err=%v writes=%d", result, err, writes)
			}
			if tc.rejected {
				requireRejectedExit(t, result, err)
				result, err = c.run(context.Background(), "operation resume", Request{Input: []byte(`{"request":"response"}`)})
				requireRejectedExit(t, result, err)
				result, err = c.run(context.Background(), "plugin edit", Request{RequestID: "response", Input: []byte(`{"id":7,"name":"same"}`)})
				requireRejectedExit(t, result, err)
				if writes != 1 {
					t.Fatalf("rejected write replayed: %d", writes)
				}
			}
		})
	}
}
