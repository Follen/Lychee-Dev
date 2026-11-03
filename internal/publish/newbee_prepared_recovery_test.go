package publish

import (
	"context"
	"encoding/json"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestNewBeeConfigRecoveryVerifiesOriginalPreparedFields(t *testing.T) {
	for _, changePreserved := range []bool{false, true} {
		t.Run(map[bool]string{false: "canonical-linked-mod", true: "preserved-field-mismatch"}[changePreserved], func(t *testing.T) {
			detail := map[string]any{"id": 7, "cloud_id": 9, "title": "original title", "content": "private original content", "sharing": 0, "linked_mods": []any{}}
			writes := 0
			client := newBeeFixture(t, func(w http.ResponseWriter, r *http.Request) {
				switch r.URL.Path {
				case "/creator/wow/share_config/details_aps":
					newBeeReply(w, detail)
				case "/v3/share/list":
					newBeeReply(w, []any{map[string]any{"t_id": 9, "t_Known_plug": []any{map[string]any{"id": 8, "name": "addon", "mod_file_id": 42, "mod_version": "1.0", "display_name": "v1", "updateType": 1}}}})
				case "/creator/wow/share_config/update":
					writes++
					var body map[string]any
					if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
						t.Error(err)
					}
					for k, v := range body {
						detail[k] = v
					}
					http.Error(w, "lost acknowledgement", http.StatusBadGateway)
				default:
					t.Errorf("unexpected endpoint %s", r.URL.Path)
				}
			})
			command := newBeeCreatorCommands["config edit"]
			input, err := command.spec.Decode([]byte(`{"id":7,"linked_mods":[{"mod_id":8}]}`))
			if err != nil {
				t.Fatal(err)
			}
			request := Request{Home: t.TempDir(), RequestID: "prepared"}
			if _, err = client.run(context.Background(), "config edit", command, input, request); err == nil {
				t.Fatal("expected lost acknowledgement")
			}
			files, err := filepath.Glob(filepath.Join(request.Home, "publish", "newbee", "*.json"))
			if err != nil || len(files) != 1 {
				t.Fatalf("original receipt: %v %v", files, err)
			}
			raw, err := os.ReadFile(files[0])
			if err != nil || strings.Contains(string(raw), "private original content") {
				t.Fatalf("receipt must retain digests without configuration content: %v", err)
			}
			if changePreserved {
				detail["content"] = "different content"
			}
			result, err := client.verifyOriginal(context.Background(), "config edit", input, request)
			if changePreserved {
				if err == nil || result.Complete {
					t.Fatalf("accepted changed preserved content: %+v %v", result, err)
				}
			} else if err != nil || !result.Complete {
				t.Fatalf("matching prepared form remains pending: %+v %v", result, err)
			}
			if writes != 1 {
				t.Fatalf("writes=%d", writes)
			}
		})
	}
}
