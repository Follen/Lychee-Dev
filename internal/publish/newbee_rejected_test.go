package publish

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"testing"
)

func TestNewBeeRejectedRenameCannotVerifyExistingValue(t *testing.T) {
	writes, reads := 0, 0
	client := newBeeFixture(t, func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/share/update":
			writes++
			json.NewEncoder(w).Encode(map[string]any{"code": 0, "data": nil})
		case "/v3/share/list":
			reads++
			newBeeReply(w, []any{map[string]any{"t_id": 7, "t_name": "existing"}})
		default:
			t.Errorf("unexpected endpoint %s", r.URL.Path)
		}
	})
	command := newBeeCreatorCommands["backup rename"]
	input, err := command.spec.Decode([]byte(`{"id":7,"name":"existing"}`))
	if err != nil {
		t.Fatal(err)
	}
	request := Request{Home: t.TempDir(), RequestID: "rejected-rename"}
	_, err = client.run(context.Background(), "backup rename", command, input, request)
	var rejected *Error
	if !errors.As(err, &rejected) || rejected.Code != "publish.newbee_business" {
		t.Fatalf("expected platform rejection: %v", err)
	}
	beforeVerify := reads
	result, err := client.verifyOriginal(context.Background(), "backup rename", input, request)
	if err == nil || result.Complete || result.Submission != "rejected" || writes != 1 || reads != beforeVerify {
		t.Fatalf("rejected operation verified from existing value: %+v %v writes=%d reads=%d/%d", result, err, writes, reads, beforeVerify)
	}
	result, err = client.run(context.Background(), "backup rename", command, input, request)
	if err == nil || result.Complete || result.Submission != "rejected" || writes != 1 {
		t.Fatalf("rejected request was replayed or lost its state: %+v %v writes=%d", result, err, writes)
	}
}

func TestNewBeeMalformedAcknowledgementRetainsUncertainRecovery(t *testing.T) {
	writes := 0
	client := newBeeFixture(t, func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/share/update":
			writes++
			w.Write([]byte(`{"data":null}`))
		case "/v3/share/list":
			newBeeReply(w, []any{map[string]any{"t_id": 7, "t_name": "renamed"}})
		default:
			t.Errorf("unexpected endpoint %s", r.URL.Path)
		}
	})
	command := newBeeCreatorCommands["backup rename"]
	input, err := command.spec.Decode([]byte(`{"id":7,"name":"renamed"}`))
	if err != nil {
		t.Fatal(err)
	}
	request := Request{Home: t.TempDir(), RequestID: "unknown-rename"}
	result, err := client.run(context.Background(), "backup rename", command, input, request)
	if err == nil || result.Submission != "uncertain" {
		t.Fatalf("malformed acknowledgement became a rejection: %+v %v", result, err)
	}
	result, err = client.verifyOriginal(context.Background(), "backup rename", input, request)
	if err != nil || !result.Complete || writes != 1 {
		t.Fatalf("unknown acknowledgement recovery failed: %+v %v writes=%d", result, err, writes)
	}
}
