package publish

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"

	"github.com/follenfang/lycheedev/internal/vault"
)

func (c *newBeeCreator) backupReferences(ctx context.Context) (map[int64][]any, error) {
	result := map[int64][]any{}
	offset := int64(0)
	total := int64(-1)
	seen := map[int64]bool{}
	for {
		value, err := c.read(ctx, "config list", map[string]any{"offset": json.Number(fmt.Sprint(offset)), "page_size": json.Number("100")})
		if err != nil {
			return nil, err
		}
		object := newBeeObject(value)
		rows := newBeeRows(value)
		rawTotal := newBeePick(object, "total", "count")
		count := newBeeID(rawTotal)
		if rawTotal == nil || count < 0 || count > 100000 || total >= 0 && total != count || rows == nil {
			return nil, failure("publish.newbee_references", "configuration reference pagination is incomplete")
		}
		total = count
		for _, row := range rows {
			obj := newBeeObject(row)
			id := newBeeID(newBeePick(obj, "id", "t_id"))
			if id <= 0 || seen[id] {
				return nil, failure("publish.newbee_references", "configuration reference IDs repeat or are missing")
			}
			seen[id] = true
			backup := newBeeID(newBeePick(obj, "cloud_id", "t_cloudblackid"))
			if backup == 0 {
				detail, err := c.read(ctx, "config get", map[string]any{"id": json.Number(fmt.Sprint(id))})
				if err != nil {
					return nil, err
				}
				record := newBeeObject(detail)
				if newBeeID(newBeePick(record, "id", "t_id")) != id {
					return nil, failure("publish.newbee_identity", "configuration reference detail has a different identity")
				}
				backup = newBeeID(newBeePick(record, "cloud_id", "t_cloudblackid"))
			}
			if backup <= 0 {
				return nil, failure("publish.newbee_references", "configuration backup reference is unknown")
			}
			result[backup] = append(result[backup], map[string]any{"id": id, "title": newBeePick(obj, "title", "t_title")})
		}
		if int64(len(seen)) == total {
			return result, nil
		}
		if len(rows) == 0 || int64(len(seen)) > total {
			return nil, failure("publish.newbee_references", "configuration reference pagination ended early")
		}
		next := offset + int64(len(rows))
		if raw, ok := object["next_offset"]; ok {
			next = newBeeID(raw)
		}
		if next <= offset {
			return nil, failure("publish.newbee_references", "configuration reference cursor did not advance")
		}
		offset = next
	}
}
func (c *newBeeCreator) backupDeletePreview(ctx context.Context, input map[string]any) (map[string]any, error) {
	selected := newBeeRows(input["ids"])
	seen := map[int64]bool{}
	for _, value := range selected {
		id := newBeeID(value)
		if id <= 0 || seen[id] {
			return nil, invalid("$.ids", "backup IDs must be positive and unique")
		}
		seen[id] = true
	}
	value, err := c.read(ctx, "backup list", nil)
	if err != nil {
		return nil, err
	}
	backups := map[int64]map[string]any{}
	for _, row := range newBeeRows(value) {
		object := newBeeObject(row)
		backups[newBeeID(newBeePick(object, "id", "t_id"))] = object
	}
	references, err := c.backupReferences(ctx)
	if err != nil {
		return nil, err
	}
	items := []any{}
	deletable := len(selected) > 0
	for _, value := range selected {
		id := newBeeID(value)
		record, present := backups[id]
		allowed := present && len(references[id]) == 0
		deletable = deletable && allowed
		items = append(items, map[string]any{"id": id, "name": newBeePick(record, "name", "t_name"), "present": present, "references": references[id], "deletable": allowed})
	}
	return map[string]any{"items": items, "deletable": deletable, "references_complete": true}, nil
}

type newBeeDeleteReceipt struct {
	Session     string  `json:"session"`
	Fingerprint string  `json:"fingerprint"`
	Completed   []int64 `json:"completed"`
	Pending     int64   `json:"pending,omitempty"`
	Rejected    bool    `json:"rejected,omitempty"`
}

func (c *newBeeCreator) runBackupDelete(ctx context.Context, input map[string]any, request Request) (Result, error) {
	result := Result{Platform: "newbee", Action: "creator backup delete"}
	if !Bool(input, "confirm_delete") {
		return result, invalid("$.confirm_delete", "must be true")
	}
	session, err := c.sessionIdentity(ctx)
	if err != nil {
		return result, err
	}
	fingerprint, err := newBeeFingerprint(ctx, result.Action, input)
	if err != nil {
		return result, err
	}
	key := sha256.Sum256([]byte(request.RequestID))
	operation := hex.EncodeToString(key[:])
	result.OperationID = operation
	file := filepath.Join(request.Home, "publish", "newbee", operation+".json")
	lease, err := vault.AcquireLease(ctx, filepath.Join(request.Home, "publish", "locks"), "newbee/"+operation)
	if err != nil {
		return result, err
	}
	defer lease.Close()
	receipt := newBeeDeleteReceipt{Session: session, Fingerprint: fingerprint}
	if raw, e := os.ReadFile(file); e == nil {
		if json.Unmarshal(raw, &receipt) != nil || receipt.Fingerprint != fingerprint {
			return result, invalid("$request", "request ID belongs to a different deletion")
		}
		if err := newBeeCheckSession(receipt.Session, session); err != nil {
			return result, err
		}
		if receipt.Rejected {
			result.Submission = "rejected"
			return result, failure("publish.newbee_business", "original backup deletion was rejected; it will not be replayed or verified from current absence")
		}
	} else if !os.IsNotExist(e) {
		return result, e
	}
	save := func() error {
		raw, _ := json.Marshal(receipt)
		if err := os.MkdirAll(filepath.Dir(file), 0700); err != nil {
			return err
		}
		return vault.ReplaceFile(ctx, file, raw)
	}
	completed := map[int64]bool{}
	for _, id := range receipt.Completed {
		completed[id] = true
	}
	if receipt.Pending != 0 {
		value, err := c.read(ctx, "backup list", nil)
		if err != nil {
			return result, err
		}
		present := false
		for _, row := range newBeeRows(value) {
			if newBeeID(newBeePick(newBeeObject(row), "id", "t_id")) == receipt.Pending {
				present = true
			}
		}
		if present {
			return result, &Error{Code: "publish.newbee_pending", Message: "original backup deletion is uncertain and the target remains present", Exit: 6}
		}
		completed[receipt.Pending] = true
		receipt.Completed = append(receipt.Completed, receipt.Pending)
		receipt.Pending = 0
		if err = save(); err != nil {
			return result, err
		}
	}
	// Validate the whole remaining selection before deleting its first item.
	remaining := []any{}
	for _, raw := range newBeeRows(input["ids"]) {
		if !completed[newBeeID(raw)] {
			remaining = append(remaining, raw)
		}
	}
	if len(remaining) > 0 {
		preview, err := c.backupDeletePreview(ctx, map[string]any{"ids": remaining})
		if err != nil {
			return result, err
		}
		if !Bool(preview, "deletable") {
			result.Data = preview
			return result, failure("publish.newbee_backup_referenced", "selected backups are absent or referenced by configurations")
		}
	}
	for _, raw := range remaining {
		id := newBeeID(raw)
		preview, err := c.backupDeletePreview(ctx, map[string]any{"ids": []any{raw}})
		if err != nil {
			return result, err
		}
		if !Bool(preview, "deletable") {
			return result, failure("publish.newbee_backup_referenced", "backup acquired a configuration reference")
		}
		receipt.Pending = id
		if err = save(); err != nil {
			return result, err
		}
		if _, err = c.backupPost(ctx, "/v3/share/delete", map[string]any{"t_id": id}); err != nil {
			if newBeeRejected(err) {
				receipt.Rejected = true
				result.Submission = "rejected"
				if saveErr := save(); saveErr != nil {
					return result, saveErr
				}
			}
			return result, err
		}
		value, err := c.read(ctx, "backup list", nil)
		if err != nil {
			return result, err
		}
		for _, row := range newBeeRows(value) {
			if newBeeID(newBeePick(newBeeObject(row), "t_id", "id")) == id {
				return result, &Error{Code: "publish.newbee_verification", Message: "deleted backup remains present", Exit: 6}
			}
		}
		receipt.Completed = append(receipt.Completed, id)
		receipt.Pending = 0
		if err = save(); err != nil {
			return result, err
		}
	}
	result.Complete = true
	result.Submission = "accepted"
	result.Verification = "absence-readback"
	result.Data = map[string]any{"deleted": receipt.Completed}
	return result, nil
}
