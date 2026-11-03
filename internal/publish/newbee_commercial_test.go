package publish

import (
	"context"
	"encoding/json"
	"net/http"
	"strings"
	"testing"
)

func TestNewBeeZeroPriceClearsDurationBeforeValidationAndRecovery(t *testing.T) {
	for _, resource := range []string{"config", "wa"} {
		t.Run(resource, func(t *testing.T) {
			detail := map[string]any{"id": 7, "price": 15, "time_range": "retired-duration", "link_to_channel": false}
			if resource == "config" {
				detail["sharing"] = 0
			} else {
				detail["share_state"] = 2
			}
			writes := 0
			client := newBeeFixture(t, func(w http.ResponseWriter, r *http.Request) {
				switch {
				case strings.HasSuffix(r.URL.Path, "/details_aps"), strings.HasSuffix(r.URL.Path, "/detail_aps"):
					newBeeReply(w, detail)
				case strings.HasSuffix(r.URL.Path, "/update"):
					writes++
					var body map[string]any
					if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
						t.Error(err)
					}
					if body["time_range"] != "" {
						t.Errorf("zero-price request retained duration: %#v", body)
					}
					for key, value := range body {
						detail[key] = value
					}
					// The platform applies the requested zero price but the acknowledgement is lost.
					detail["time_range"] = ""
					http.Error(w, "lost acknowledgement", http.StatusBadGateway)
				default:
					t.Errorf("unexpected remote validation: %s", r.URL.Path)
					http.Error(w, "unexpected", http.StatusBadRequest)
				}
			})
			raw := `{"id":7,"price":0}`
			if resource == "wa" {
				raw = `{"id":7,"price":0,"time_range":"retired-duration"}`
			}
			command := newBeeCreatorCommands[resource+" edit"]
			input, err := command.spec.Decode([]byte(raw))
			if err != nil {
				t.Fatal(err)
			}
			request := Request{Home: t.TempDir(), RequestID: "zero-price"}
			if _, err = client.run(context.Background(), resource+" edit", command, input, request); err == nil {
				t.Fatal("expected lost acknowledgement")
			}
			result, err := client.verifyOriginal(context.Background(), resource+" edit", input, request)
			if err != nil || !result.Complete || writes != 1 {
				t.Fatalf("recovery=%+v err=%v writes=%d", result, err, writes)
			}
		})
	}
}
