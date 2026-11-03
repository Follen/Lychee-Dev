package publish

import (
	"context"
	"encoding/json"
	"net/http"
	"testing"
)

func TestNewBeeBackupListRetainsEveryServerGroup(t *testing.T) {
	client := newBeeFixture(t, func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/v3/share/list" {
			t.Errorf("unexpected endpoint %s", r.URL.Path)
		}
		newBeeReply(w, []any{[]any{map[string]any{"t_id": 1}}, []any{[]any{map[string]any{"t_id": 2}}}})
	})
	value, err := client.read(context.Background(), "backup list", nil)
	if err != nil {
		t.Fatal(err)
	}
	rows := newBeeRows(value)
	if len(rows) != 2 {
		t.Errorf("lost grouped backups: %#v", value)
	}
	value, err = client.read(context.Background(), "backup get", map[string]any{"id": json.Number("2")})
	if err != nil || newBeeID(newBeeObject(value)["t_id"]) != 2 {
		t.Fatalf("second-group backup: %#v %v", value, err)
	}
}
