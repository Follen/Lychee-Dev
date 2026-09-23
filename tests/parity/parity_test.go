package parity

import (
	"bytes"
	"encoding/json"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"testing"
)

type ledger struct {
	Schema      string   `json:"schema"`
	Purpose     string   `json:"purpose"`
	Offline     bool     `json:"offline"`
	Groups      []group  `json:"groups"`
	Normalizers []string `json:"normalizers"`
}

type group struct {
	Legacy   string   `json:"legacy"`
	Current  []string `json:"current"`
	Evidence []string `json:"evidence"`
	Status   string   `json:"status"`
}

func TestCoverageLedgerIsCompleteAndGrounded(t *testing.T) {
	root := filepath.Clean(filepath.Join("..", ".."))
	raw, err := os.ReadFile("coverage.json")
	if err != nil {
		t.Fatal(err)
	}
	var doc ledger
	dec := json.NewDecoder(bytes.NewReader(raw))
	dec.DisallowUnknownFields()
	if err := dec.Decode(&doc); err != nil {
		t.Fatal(err)
	}
	if doc.Schema != "lycheedev.semantic-parity-coverage.v1" || !doc.Offline {
		t.Fatalf("unexpected ledger contract: %+v", doc)
	}
	seen := map[string]bool{}
	for _, g := range doc.Groups {
		if g.Legacy == "" || seen[g.Legacy] || len(g.Current) == 0 || len(g.Evidence) == 0 {
			t.Fatalf("incomplete or duplicate group: %+v", g)
		}
		seen[g.Legacy] = true
		if g.Status != "fixture-backed" && g.Status != "fixture-backed-partial" && g.Status != "current-tests" && g.Status != "intentional-change" {
			t.Fatalf("unknown status %q", g.Status)
		}
		for _, pattern := range g.Evidence {
			matches, err := filepath.Glob(filepath.Join(root, filepath.FromSlash(pattern)))
			if err != nil || len(matches) == 0 {
				t.Errorf("%s evidence %q has no repository match (err=%v)", g.Legacy, pattern, err)
			}
		}
	}
	for _, want := range []string{"wowdoc.source", "wowdata.sql", "wowdata.db2", "wowdata.hotfix", "wowdata.video", "wowdata.cache", "wowdata.golden"} {
		if !seen[want] {
			t.Errorf("legacy group missing from ledger: %s", want)
		}
	}
	wantNormalizers := []string{"cache-state-v1", "profile-state-v1", "remote-products-v1"}
	got := append([]string(nil), doc.Normalizers...)
	sort.Strings(got)
	if strings.Join(got, ",") != strings.Join(wantNormalizers, ",") {
		t.Fatalf("normalizer set = %v, want %v", got, wantNormalizers)
	}
}

// normalize removes only documented volatile fields and canonicalizes object
// ordering. It deliberately preserves semantic arrays and all unknown fields.
func normalize(name string, value any) (any, bool) {
	obj, ok := value.(map[string]any)
	if !ok {
		return value, false
	}
	copy := make(map[string]any, len(obj))
	for k, v := range obj {
		copy[k] = v
	}
	switch name {
	case "remote-products-v1":
		delete(copy, "fetchedAt")
		delete(copy, "requestId")
	case "cache-state-v1":
		delete(copy, "measuredAt")
		delete(copy, "durationMs")
	case "profile-state-v1":
		delete(copy, "updatedAt")
		delete(copy, "revision")
	default:
		return value, false
	}
	for k, v := range copy {
		if child, ok := v.(map[string]any); ok {
			copy[k], _ = normalize(name, child)
		}
	}
	return copy, true
}

func TestSemanticNormalizersIgnoreOnlyDeclaredVolatility(t *testing.T) {
	cases := []struct{ name, left, right string }{
		{"remote-products-v1", `{"products":["wow","wow_classic"],"fetchedAt":"a","requestId":"1"}`, `{"requestId":"2","fetchedAt":"b","products":["wow","wow_classic"]}`},
		{"cache-state-v1", `{"bytes":12,"measuredAt":"a","durationMs":4}`, `{"durationMs":9,"measuredAt":"b","bytes":12}`},
		{"profile-state-v1", `{"name":"retail","build":"120100","updatedAt":"a","revision":1}`, `{"revision":2,"updatedAt":"b","build":"120100","name":"retail"}`},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			var a, b any
			if err := json.Unmarshal([]byte(tc.left), &a); err != nil {
				t.Fatal(err)
			}
			if err := json.Unmarshal([]byte(tc.right), &b); err != nil {
				t.Fatal(err)
			}
			na, ok := normalize(tc.name, a)
			if !ok {
				t.Fatal("normalizer rejected known family")
			}
			nb, _ := normalize(tc.name, b)
			ja, _ := json.Marshal(na)
			jb, _ := json.Marshal(nb)
			if !bytes.Equal(ja, jb) {
				t.Fatalf("semantically equal values differ:\n%s\n%s", ja, jb)
			}
			changed := strings.Replace(tc.right, `"120100"`, `"120101"`, 1)
			if tc.name != "profile-state-v1" {
				changed = strings.Replace(tc.right, `"wow_classic"`, `"wow_other"`, 1)
			}
			if tc.name == "cache-state-v1" {
				changed = strings.Replace(tc.right, `"bytes":12`, `"bytes":13`, 1)
			}
			var c any
			_ = json.Unmarshal([]byte(changed), &c)
			nc, _ := normalize(tc.name, c)
			jc, _ := json.Marshal(nc)
			if bytes.Equal(ja, jc) {
				t.Fatal("semantic field change was normalized away")
			}
		})
	}
}
