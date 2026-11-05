package publish

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"os"
	"path/filepath"
	"strings"

	"github.com/follenfang/lycheedev/internal/vault"
)

func newBeeDecodeStored(raw []byte, value any) error {
	if bytes.Equal(bytes.TrimSpace(raw), []byte("null")) {
		return errors.New("null original request receipt")
	}
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.UseNumber()
	if err := decoder.Decode(value); err != nil {
		return err
	}
	if decoder.Decode(new(any)) != io.EOF {
		return errors.New("original request receipt contains trailing data")
	}
	return nil
}

func newBeeOperationStatus(request Request) (Result, error) {
	hash := sha256.Sum256([]byte(request.RequestID))
	operation := hex.EncodeToString(hash[:])
	result := Result{Platform: "newbee", Action: "operation status", OperationID: operation}
	raw, err := os.ReadFile(filepath.Join(request.Home, "publish", "newbee", operation+".json"))
	if err != nil {
		return result, capability("publish.newbee_receipt", "original request receipt was not found")
	}
	var receipt map[string]any
	if newBeeDecodeStored(raw, &receipt) != nil {
		return result, failure("publish.newbee_receipt", "invalid original request receipt")
	}
	fingerprint, err := hex.DecodeString(String(receipt, "fingerprint"))
	if err != nil || len(fingerprint) != sha256.Size {
		return result, failure("publish.newbee_receipt", "invalid original request fingerprint")
	}
	data := map[string]any{}
	for _, key := range []string{"state", "stage", "uploaded", "id", "complete", "completed", "pending", "rejected", "result", "target"} {
		if value, ok := receipt[key]; ok {
			data[key] = newBeePublic(value)
		}
	}
	result.Complete = true
	result.Data = data
	return result, nil
}
func (c *newBeeCreator) verifyOriginal(ctx context.Context, action string, input map[string]any, request Request) (Result, error) {
	result := Result{Platform: "newbee", Action: "creator " + action + " verify"}
	session, err := c.sessionIdentity(ctx)
	if err != nil {
		return result, err
	}
	fingerprint, err := newBeeFingerprint(ctx, "creator "+action, input)
	if err != nil {
		return result, err
	}
	hash := sha256.Sum256([]byte(request.RequestID))
	operation := hex.EncodeToString(hash[:])
	result.OperationID = operation
	path := filepath.Join(request.Home, "publish", "newbee", operation+".json")
	lease, err := vault.AcquireLease(ctx, filepath.Join(request.Home, "publish", "locks"), "newbee/"+operation)
	if err != nil {
		return result, err
	}
	defer lease.Close()
	raw, err := os.ReadFile(path)
	if err != nil {
		return result, capability("publish.newbee_receipt", "original request receipt was not found")
	}
	var receipt newBeeReceipt
	if newBeeDecodeStored(raw, &receipt) != nil {
		return result, failure("publish.newbee_receipt", "invalid original request receipt")
	}
	if receipt.Fingerprint != fingerprint {
		return result, invalid("$request", "original input and selected file bytes must match the request")
	}
	if err := newBeeCheckSession(receipt.Session, session); err != nil {
		return result, err
	}
	if receipt.State == "complete" {
		return receipt.Result, nil
	}
	if receipt.State == "rejected" {
		result.Submission = "rejected"
		return result, failure("publish.newbee_business", "original Creator operation was rejected; matching current fields do not verify that operation")
	}
	command := newBeeCreatorCommands[action]
	body := newBeeBody(command, input)
	resource, verb, _ := strings.Cut(action, " ")
	if verb == "edit" || verb == "create" {
		delete(body, "id")
		if v, ok := body["picture_urls"]; ok {
			body["pic_url"] = v
			delete(body, "picture_urls")
		}
		if v, ok := body["public"].(bool); ok {
			delete(body, "public")
			key := "share_state"
			value := 0
			if v {
				value = 1
			}
			if resource == "wa" && !v {
				value = 2
			}
			if resource == "config" {
				key = "sharing"
			}
			if resource == "plugin" && verb == "create" {
				value = 0
			}
			body[key] = value
		}
		newBeeNormalizePrice(body)
	}
	readback, err := c.verify(ctx, action, input, body, receipt.Result.Data, receipt.Expected)
	if err != nil {
		return result, err
	}
	result.Complete = true
	result.Submission = "accepted"
	result.Verification = "metadata-readback"
	result.Data = map[string]any{"readback": newBeePublic(readback), "resubmitted": false}
	receipt.State = "complete"
	receipt.Result = result
	raw, _ = json.Marshal(receipt)
	if err = vault.ReplaceFile(ctx, path, raw); err != nil {
		result.Complete = false
		return result, err
	}
	return result, nil
}
