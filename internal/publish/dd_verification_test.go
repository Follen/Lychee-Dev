package publish

import (
	"context"
	"errors"
	"net/url"
	"testing"
)

func TestDDRejectedMutationCannotVerifyMatchingExistingResource(t *testing.T) {
	ctx := context.Background()
	home := t.TempDir()
	writes := 0
	api := &ddFake{fn: func(path string, _ url.Values, _ any, mutation bool) (map[string]any, error) {
		if mutation {
			writes++
			return nil, &Error{Code: "publish.dd_rejected", Message: "synthetic rejection", Exit: 5}
		}
		if path == "/backup/list" {
			return ddEnvelope([]any{map[string]any{"sn": "backup-1"}}), nil
		}
		return ddEnvelope(map[string]any{"sn": "backup-1", "name": "Existing"}), nil
	}}
	req := Request{Home: home, RequestID: "rejected-rename"}
	result, err := ddMutationRun(ctx, api, req, "backup rename", map[string]any{"sn": "backup-1", "name": "Existing"})
	if err == nil || result.Submission != "rejected" || result.Complete || writes != 1 {
		t.Fatalf("rejected mutation: %#v %v writes=%d", result, err, writes)
	}
	r, err := ddLoad(home, req.RequestID)
	if err != nil {
		t.Fatal(err)
	}
	result, err = ddVerify(ctx, api, home, r, "operation verify")
	var pe *Error
	if !errors.As(err, &pe) || result.Complete || result.Verification == "verified" || result.Submission != "rejected" || writes != 1 {
		t.Fatalf("rejection became success: %#v %v writes=%d", result, err, writes)
	}
}

func TestDDVerificationRequiresSubmittedOperation(t *testing.T) {
	for _, submission := range []string{"intent_recorded", "rejected", "uncertain", "acknowledged"} {
		t.Run(submission, func(t *testing.T) {
			ctx := context.Background()
			home := t.TempDir()
			api := &ddFake{fn: func(path string, _ url.Values, _ any, mutation bool) (map[string]any, error) {
				if mutation || path != "/backup/detail" {
					t.Fatalf("unexpected verify call %s mutation=%v", path, mutation)
				}
				return ddEnvelope(map[string]any{"sn": "backup-1", "name": "Existing"}), nil
			}}
			r := ddReceipt{Schema: "lycheedev.publish.dd-operation.v1", Request: "original-rename", Author: "author-1", Action: "backup rename", SN: "backup-1", Submission: submission, Verification: "pending", Expected: map[string]any{"sn": "backup-1", "name": "Existing"}}
			if err := ddSave(ctx, home, r); err != nil {
				t.Fatal(err)
			}
			result, err := ddVerify(ctx, api, home, r, "operation verify")
			shouldVerify := submission == "uncertain" || submission == "acknowledged"
			if shouldVerify {
				if err != nil || !result.Complete || api.calls != 1 {
					t.Fatalf("submitted operation failed readback: %#v %v calls=%d", result, err, api.calls)
				}
			} else if err == nil || result.Complete || api.calls != 0 {
				t.Fatalf("unsubmitted operation read back: %#v %v calls=%d", result, err, api.calls)
			}
			stored, err := ddLoad(home, r.Request)
			if err != nil || (stored.Verification == "verified") != shouldVerify || stored.Submission != submission {
				t.Fatalf("original receipt: %#v %v", stored, err)
			}
		})
	}
}
