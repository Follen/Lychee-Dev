package command

import (
	"encoding/json"
	"strings"
	"testing"
)

func TestDataQueryJSONExplicitHotfix(t *testing.T) {
	q, err := decodeDataQuery([]byte(`{"sql":"SELECT ID FROM effective.Sample WHERE ID=:id","parameters":{"id":9007199254740993},"hotfix":["CAP-first","CAP-second"]}`))
	if err != nil || len(q.Hotfix) != 2 || q.Hotfix[1] != "CAP-second" || q.Parameters["id"] != json.Number("9007199254740993") {
		t.Fatalf("%+v %v", q, err)
	}
	for _, raw := range []string{
		`{"sql":"SELECT ID FROM Sample","hotfix":[]}`,
		`{"sql":"SELECT ID FROM Sample","hotfix":null}`,
		`{"sql":"SELECT ID FROM Sample","hotfix":[42]}`,
		`{"sql":"SELECT ID FROM Sample","hotfix":["a"],"hotfix":["b"]}`,
		`{"sql":"SELECT ID FROM Sample","hotfix":[` + strings.Repeat(`"a",`, 16) + `"a"]}`,
	} {
		if _, err := decodeDataQuery([]byte(raw)); err == nil {
			t.Fatalf("accepted invalid query: %s", raw)
		}
	}
}
