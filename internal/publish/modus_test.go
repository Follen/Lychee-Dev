package publish

import (
	"archive/zip"
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
)

func modusTestZIP(t *testing.T, files map[string]string) artifact {
	t.Helper()
	filename := filepath.Join(t.TempDir(), "addon.zip")
	f, err := os.Create(filename)
	if err != nil {
		t.Fatal(err)
	}
	z := zip.NewWriter(f)
	for name, text := range files {
		w, err := z.Create(name)
		if err != nil {
			t.Fatal(err)
		}
		if _, err = w.Write([]byte(text)); err != nil {
			t.Fatal(err)
		}
	}
	if err = z.Close(); err != nil {
		t.Fatal(err)
	}
	if err = f.Close(); err != nil {
		t.Fatal(err)
	}
	a, err := inspectArtifact(context.Background(), filename)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { a.file.Close() })
	return a
}
func TestModusZIPClientUnionAndLiveSelection(t *testing.T) {
	a := modusTestZIP(t, map[string]string{"A/A.toc": "## Interface: 120100\n", "A/A_Mainline.toc": "## Interface: 120105\n", "B/B.toc": "## Interface: 120100,120105\n", "A/Libs/L/L.toc": "## Interface: 99999\n"})
	m, err := inspectModusZIP(context.Background(), a)
	if err != nil {
		t.Fatal(err)
	}
	if m.TOCVersion != "120100,120105" || len(m.Games) != 2 || len(m.TOCFiles) != 3 {
		t.Fatalf("unexpected metadata: %+v", m)
	}
	config := []any{map[string]any{"key": "wow_builds", "value": `{"wow_retail":{"versions":["12.1.0","12.1.5","12.2.0"]},"wow_classic":{"versions":["5.5.4"]}}`}}
	got, err := selectModusGames(m.Games, config, nil)
	if err != nil || !reflect.DeepEqual(got, m.Games) {
		t.Fatalf("default changed builds: %+v %v", got, err)
	}
	explicit := []modusGameVersion{{"12.2.0", "wow_retail"}}
	got, err = selectModusGames(m.Games, config, explicit)
	if err != nil || !reflect.DeepEqual(got, explicit) {
		t.Fatalf("explicit build failed: %+v %v", got, err)
	}
	for _, bad := range [][]modusGameVersion{{}, {{"5.5.4", "wow_classic"}}, {{"12.1.5", "wow_retail"}, {"12.1.5", "wow_retail"}}} {
		if _, err = selectModusGames(m.Games, config, bad); err == nil {
			t.Fatalf("accepted invalid choices: %+v", bad)
		}
	}
}
func TestModusZIPRejectsAmbiguousOrUnknownInterfaces(t *testing.T) {
	for _, files := range []map[string]string{
		{"A/A.toc": "## Interface: 120105", "B/B.toc": "## Interface: 50504"},
		{"A/A.toc": "## Interface: 99999"},
		{"A/A.toc": "## Interface: "},
		{"A/A.toc": "## Interface: 120105", "A/a.toc": "## Interface: 120105"},
	} {
		a := modusTestZIP(t, files)
		if _, err := inspectModusZIP(context.Background(), a); err == nil {
			t.Fatalf("accepted %v", files)
		}
	}
	a := modusTestZIP(t, map[string]string{"A/A.toc": "## Interface: 30405,16001"})
	m, err := inspectModusZIP(context.Background(), a)
	if err != nil || len(m.Unmapped) != 1 || len(m.Games) != 1 || m.Games[0].Server != "wow_forever" {
		t.Fatalf("platform identities lost: %+v %v", m, err)
	}
}
func TestModusReadProtocolIdentityAndRedaction(t *testing.T) {
	var got map[string]any
	wrong := false
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Header.Get("Authorization") != "Bearer creator-test" {
			t.Errorf("wrong credential scheme")
		}
		switch r.URL.Path {
		case "/game/data/author/project/list":
			if r.Method != "POST" {
				t.Errorf("method %s", r.Method)
			}
			if err := json.NewDecoder(r.Body).Decode(&got); err != nil {
				t.Error(err)
			}
			w.Write([]byte(`{"code":200,"data":{"rows":[],"total":0}}`))
		case "/game/data/author/project/file/detail/9":
			if wrong {
				w.Write([]byte(`{"data":{"projectId":200000002,"fileId":9}}`))
			} else {
				w.Write([]byte(`{"data":{"projectId":200000001,"fileId":9,"signedUrl":"secret-url"}}`))
			}
		default:
			t.Errorf("unexpected route %s", r.URL.Path)
		}
	}))
	defer srv.Close()
	c := &modusClient{http: NewHTTP(), origin: srv.URL + "/", token: "creator-test"}
	_, err := c.read(context.Background(), "project list", map[string]any{"ascending": false, "page_num": json.Number("2"), "page_size": json.Number("10")})
	if err != nil || got["isAsc"] != false || got["pageNum"] != float64(2) {
		t.Fatalf("paging or false lost: %v %v", got, err)
	}
	input := map[string]any{"project_id": json.Number("200000001"), "file_id": json.Number("9")}
	v, err := c.read(context.Background(), "plugin get", input)
	if err != nil {
		t.Fatal(err)
	}
	raw, _ := json.Marshal(modusSafe(v))
	if strings.Contains(string(raw), "secret-url") {
		t.Fatal("signed URL leaked")
	}
	wrong = true
	if _, err = c.read(context.Background(), "plugin get", input); err == nil {
		t.Fatal("accepted foreign project detail")
	}
}

func TestModusProjectPartialEditPreservesExclusiveCategoryConstraint(t *testing.T) {
	for _, tc := range []struct {
		name, input, current string
	}{
		{"change_category", `{"project_id":200000001,"categories":[998]}`, `{"projectId":200000001,"categories":"1","synchronizationType":1}`},
		{"change_platform", `{"project_id":200000001,"publish_platforms":["modus"]}`, `{"projectId":200000001,"categories":[{"id":998}],"synchronizationType":2}`},
	} {
		t.Run(tc.name, func(t *testing.T) {
			srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				if strings.Contains(r.URL.Path, "/detail/") {
					w.Write([]byte(`{"code":200,"data":` + tc.current + `}`))
				} else {
					w.Write([]byte(`{"code":200,"data":[{"id":998}]}`))
				}
			}))
			defer srv.Close()
			c := &modusClient{http: NewHTTP(), origin: srv.URL + "/"}
			spec, _ := ModusInput("project edit")
			in, err := spec.Decode([]byte(tc.input))
			if err != nil {
				t.Fatal(err)
			}
			if _, err := c.prepareProject(context.Background(), in, false); err == nil {
				t.Fatal("accepted category 998 with effective ModUs publication")
			}
			in["categories"] = []any{json.Number("998")}
			in["publish_platforms"] = []any{"bigfoot"}
			if _, err := c.prepareProject(context.Background(), in, false); err != nil {
				t.Fatalf("valid explicit BigFoot selection: %v", err)
			}
		})
	}
}
