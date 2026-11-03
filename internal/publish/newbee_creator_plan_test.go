package publish

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"testing"
)

func TestNewBeeCreatorPlanRelationshipAdmission(t *testing.T) {
	for _, resource := range []string{"plugin", "config", "wa"} {
		for _, tc := range []struct {
			name, verb, input string
			valid             bool
		}{
			{"duplicate-author", "co-author-set", `{"id":7,"co_authors":[{"user_id":1,"share_percent":20},{"user_id":1,"share_percent":30}]}`, false},
			{"excess-share", "co-author-set", `{"id":7,"co_authors":[{"user_id":1,"share_percent":60},{"user_id":2,"share_percent":50}]}`, false},
			{"duplicate-reference", "reference-set", `{"id":7,"references":[{"type":1,"id":9},{"type":1,"id":9}]}`, false},
			{"full-share", "co-author-set", `{"id":7,"co_authors":[{"user_id":1,"share_percent":60},{"user_id":2,"share_percent":40}]}`, true},
			{"distinct-reference-types", "reference-set", `{"id":7,"references":[{"type":1,"id":9},{"type":2,"id":9}]}`, true},
			{"clear", "co-author-set", `{"id":7,"co_authors":[]}`, true},
		} {
			t.Run(resource+"/"+tc.name, func(t *testing.T) {
				result, err := RunNewBee(context.Background(), "creator "+resource+" "+tc.verb, Request{Plan: true, RequestID: "plan", Input: []byte(tc.input)})
				if tc.valid {
					if err != nil || !result.Complete {
						t.Fatalf("valid relationship rejected: %+v %v", result, err)
					}
				} else if err == nil || result.Complete {
					t.Fatalf("invalid relationship accepted: %+v %v", result, err)
				}
			})
		}
	}
}

func TestNewBeeWAChangelogEditRequiresSelectedWAMembership(t *testing.T) {
	for _, tc := range []struct {
		valid bool
		page  int
	}{{false, 1}, {true, 1}, {true, 2}} {
		t.Run(fmt.Sprint(tc), func(t *testing.T) {
			valid := tc.valid
			writes := 0
			content := "old"
			client := newBeeFixture(t, func(w http.ResponseWriter, r *http.Request) {
				switch r.URL.Path {
				case "/creator/wow/wa_log/list":
					var request map[string]json.Number
					if err := json.NewDecoder(r.Body).Decode(&request); err != nil {
						t.Error(err)
						return
					}
					if request["wa_id"].String() != "7" {
						t.Errorf("wrong WA: %v", request)
					}
					page, _ := request["pagenum"].Int64()
					rows := []any{}
					if tc.page == 2 && page == 1 {
						for i := int64(100); i < 200; i++ {
							rows = append(rows, map[string]any{"id": i, "content": "other"})
						}
					}
					if valid && int(page) == tc.page {
						rows = append(rows, map[string]any{"id": 9, "content": content})
					}
					total := len(rows)
					if tc.page == 2 {
						total = 101
					}
					newBeeReply(w, map[string]any{"list": rows, "total": total})
				case "/creator/wow/wa_log/edit":
					writes++
					content = "new"
					newBeeReply(w, map[string]any{})
				default:
					t.Errorf("unexpected endpoint %s", r.URL.Path)
					http.Error(w, "unexpected", 500)
				}
			})
			command := newBeeCreatorCommands["wa changelog-edit"]
			input, err := command.spec.Decode([]byte(`{"id":9,"wa_id":7,"wa_log":"new"}`))
			if err != nil {
				t.Fatal(err)
			}
			result, err := client.run(context.Background(), "wa changelog-edit", command, input, Request{Home: t.TempDir(), RequestID: "edit-log"})
			if valid {
				if err != nil || !result.Complete || writes != 1 {
					t.Fatalf("valid edit: %+v %v writes=%d", result, err, writes)
				}
			} else if err == nil || writes != 0 {
				t.Fatalf("foreign log modified: %+v %v writes=%d", result, err, writes)
			}
		})
	}
}

func TestNewBeePluginChangelogReadbackBindsFileAndClear(t *testing.T) {
	for _, tc := range []struct {
		name, text string
		readback   map[string]any
		valid      bool
	}{
		{"matching", "notes", map[string]any{"file_id": 9, "changelog": "notes"}, true},
		{"foreign-file", "notes", map[string]any{"file_id": 10, "changelog": "notes"}, false},
		{"missing-file", "notes", map[string]any{"changelog": "notes"}, false},
		{"missing-content", "", map[string]any{"file_id": 9}, false},
		{"cleared-null", "", map[string]any{"file_id": "9", "changelog": nil}, true},
	} {
		t.Run(tc.name, func(t *testing.T) {
			writes := 0
			client := newBeeFixture(t, func(w http.ResponseWriter, r *http.Request) {
				switch r.URL.Path {
				case "/creator/wow/mod_file/edit_changelog":
					writes++
					newBeeReply(w, map[string]any{})
				case "/creator/wow/mod_file/get_changelog":
					newBeeReply(w, tc.readback)
				default:
					t.Errorf("unexpected endpoint %s", r.URL.Path)
				}
			})
			command := newBeeCreatorCommands["plugin changelog-edit"]
			raw, _ := json.Marshal(map[string]any{"id": 9, "changelog": tc.text})
			input, err := command.spec.Decode(raw)
			if err != nil {
				t.Fatal(err)
			}
			request := Request{Home: t.TempDir(), RequestID: "changelog"}
			result, err := client.run(context.Background(), "plugin changelog-edit", command, input, request)
			if result.Complete != tc.valid || (err == nil) != tc.valid || writes != 1 {
				t.Fatalf("initial readback: %+v %v writes=%d", result, err, writes)
			}
			result, err = client.verifyOriginal(context.Background(), "plugin changelog-edit", input, request)
			if result.Complete != tc.valid || (err == nil) != tc.valid || writes != 1 {
				t.Fatalf("original readback: %+v %v writes=%d", result, err, writes)
			}
		})
	}
}
