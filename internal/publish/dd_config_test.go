package publish

import (
	"context"
	"encoding/json"
	"net/url"
	"testing"
	"time"
)

// The pinned DD editor projects boolean purchase flags and null empty groups
// before comparing configuration readback (dd.py config_readback_projection).
func TestDDConfigReadbackWireProjection(t *testing.T) {
	for _, paid := range []bool{false, true} {
		t.Run(map[bool]string{false: "free", true: "paid"}[paid], func(t *testing.T) {
			expected := map[string]any{"need_buy": json.Number("0"), "wtf": map[string]any{"accounts": []any{}}, "vip_levels": []any{}}
			if paid {
				expected["need_buy"] = json.Number("1")
			}
			actual := map[string]any{"sn": "share-1", "need_buy": paid, "wtf": nil, "vip_levels": nil}
			for _, k := range []string{"known_addon", "unknown_addon", "material", "font", "known_wa", "unknown_wa"} {
				expected[k] = map[string]any{"items": []any{}, "inner_version": map[string]any{}}
				actual[k] = nil
			}
			api := &ddFake{fn: func(path string, q url.Values, body any, mutation bool) (map[string]any, error) {
				if path != "/share/detail" || q.Get("sn") != "share-1" || mutation {
					t.Fatalf("unexpected call %s %v %v", path, q, mutation)
				}
				return ddEnvelope(actual), nil
			}}
			home := t.TempDir()
			r := ddReceipt{Schema: "lycheedev.publish.dd-operation.v1", Request: "config-readback", Author: "author-1", Action: "config edit", SN: "share-1", Expected: expected, Submission: "acknowledged", Verification: "pending", Created: time.Now()}
			got, err := ddVerify(context.Background(), api, home, r, "operation verify")
			if err != nil || !got.Complete {
				t.Fatalf("equivalent DD wire readback remained pending: %#v %v", got, err)
			}
			actual["need_buy"] = !paid
			got, err = ddVerify(context.Background(), api, home, r, "operation verify")
			if err == nil || got.Complete {
				t.Fatalf("different purchase mode verified: %#v %v", got, err)
			}
		})
	}
}
