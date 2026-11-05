//go:build windows

package publish

import (
	"context"
	"errors"
	"fmt"
	"net/http"
	"reflect"
	"strconv"
	"testing"
)

func bbVersionHistory(size int) []any {
	rows := make([]any, size)
	for i := range rows {
		rows[i] = map[string]any{"id": i + 1, "moduleId": 7, "auditState": 2}
	}
	return rows
}

func TestBlackboxVersionZeroTotalRejectsInvalidContinuation(t *testing.T) {
	rows := bbVersionHistory(134)
	for _, test := range []struct {
		name string
		page map[string]any
	}{
		{"empty", map[string]any{"versionList": []any{}, "total": 0}},
		{"duplicate", map[string]any{"versionList": rows[:33], "total": 0}},
		{"overflow", map[string]any{"versionList": rows[100:], "total": 0}},
		{"conflicting fields", map[string]any{"versionList": rows[100:133], "total": 0, "count": 133}},
		{"multiple zero fields", map[string]any{"versionList": rows[100:133], "total": 0, "count": 0}},
		{"other count field zero", map[string]any{"versionList": rows[100:133], "total_count": 0}},
		{"changed nonzero", map[string]any{"versionList": rows[100:133], "total": 134}},
		{"string count", map[string]any{"versionList": rows[100:133], "total": "0"}},
		{"negative count", map[string]any{"versionList": rows[100:133], "total": -1}},
		{"invalid id", map[string]any{"versionList": []any{map[string]any{"id": 0}}, "total": 0}},
		{"foreign module", map[string]any{"versionList": []any{map[string]any{"id": 101, "moduleId": 8}}, "total": 0}},
	} {
		t.Run(test.name, func(t *testing.T) {
			c := bbFixtureClient(t)
			c.http.Transport = bbTestTransport(func(r *http.Request) (*http.Response, error) {
				if r.Method != "GET" || r.URL.Path != "/wow/open_platform/module_version/list/" {
					t.Fatalf("unexpected request: %s %s", r.Method, r.URL.Path)
				}
				if r.URL.Query().Get("offset") == "0" {
					return bbReply(map[string]any{"versionList": rows[:100], "total": 133}), nil
				}
				return bbReply(test.page), nil
			})
			if _, _, err := c.versions(context.Background(), "7"); err == nil {
				t.Fatal("invalid continuation accepted")
			}
		})
	}
}

func TestBlackboxVersionDeleteRejectsTargetOnZeroTotalContinuation(t *testing.T) {
	c := bbFixtureClient(t)
	rows := bbVersionHistory(133)
	var offsets []int
	c.http.Transport = bbTestTransport(func(r *http.Request) (*http.Response, error) {
		if r.Method != "GET" || r.URL.Path != "/wow/open_platform/module_version/list/" {
			t.Fatalf("verification must only read: %s %s", r.Method, r.URL.Path)
		}
		offset, _ := strconv.Atoi(r.URL.Query().Get("offset"))
		offsets = append(offsets, offset)
		total := 0
		if offset == 0 {
			total = len(rows)
		}
		return bbReply(map[string]any{"versionList": rows[offset:min(offset+100, len(rows))], "total": total}), nil
	})
	op := &bbOperation{Action: "version delete", Input: map[string]any{"module_id": 7}, Target: "133", Steps: []bbStep{{State: "submitted"}}}
	err := c.verify(context.Background(), op)
	var fault *Error
	if !errors.As(err, &fault) || fault.Code != bbVerification().(*Error).Code || op.Verified {
		t.Fatalf("undeleted continuation target: verified=%v error=%v", op.Verified, err)
	}
	if !reflect.DeepEqual(offsets, []int{0, 100}) {
		t.Fatalf("verification did not inspect the continuation: %v", offsets)
	}
}

func TestBlackboxVersionContinuationZeroTotal(t *testing.T) {
	for _, size := range []int{133, 173, 205} {
		for _, action := range []string{"plugin get", "plugin versions"} {
			t.Run(fmt.Sprintf("%d/%s", size, action), func(t *testing.T) {
				c := bbFixtureClient(t)
				rows := bbVersionHistory(size)
				var offsets []int
				c.http.Transport = bbTestTransport(func(r *http.Request) (*http.Response, error) {
					if r.Method != "GET" {
						t.Fatalf("unexpected write: %s %s", r.Method, r.URL.Path)
					}
					switch r.URL.Path {
					case "/wow/open_platform/module/detail/":
						return bbReply(map[string]any{"module": map[string]any{"id": 7}}), nil
					case "/wow/open_platform/module_version/list/":
						offset, err := strconv.Atoi(r.URL.Query().Get("offset"))
						if err != nil || offset < 0 || offset >= size || r.URL.Query().Get("limit") != "100" {
							t.Fatalf("unexpected pagination: %s", r.URL.RawQuery)
						}
						offsets = append(offsets, offset)
						total := 0
						if offset == 0 {
							total = size
						}
						return bbReply(map[string]any{"versionList": rows[offset:min(offset+100, size)], "total": total}), nil
					default:
						t.Fatalf("unexpected request: %s", r.URL.Path)
						return nil, nil
					}
				})
				value, err := c.read(context.Background(), action, map[string]any{"module_id": 7})
				if err != nil {
					t.Fatal(err)
				}
				versions := value.(map[string]any)["versions"].([]map[string]any)
				if len(versions) != size || bbID(versions[size-1]["id"]) != strconv.Itoa(size) {
					t.Fatalf("incomplete history: %d rows", len(versions))
				}
				var want []int
				for offset := 0; offset < size; offset += 100 {
					want = append(want, offset)
				}
				if !reflect.DeepEqual(offsets, want) {
					t.Fatalf("offsets=%v, want %v", offsets, want)
				}
			})
		}
	}
}
