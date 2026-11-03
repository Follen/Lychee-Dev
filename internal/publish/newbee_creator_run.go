package publish

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"net/http"
	"os"
	"path/filepath"
	"reflect"
	"strconv"
	"strings"

	"github.com/follenfang/lycheedev/internal/vault"
)

func newBeeBody(c newBeeCreatorCommand, input map[string]any) map[string]any {
	body := map[string]any{}
	for k, v := range c.defaults {
		body[k] = v
	}
	for k, v := range input {
		if target, ok := c.mapping[k]; ok {
			body[target] = v
		}
	}
	return body
}
func (c *newBeeCreator) read(ctx context.Context, action string, input map[string]any) (any, error) {
	command := newBeeCreatorCommands[action]
	if action == "session doctor" {
		if err := c.authenticate(ctx); err != nil {
			return nil, err
		}
		return map[string]any{"authenticated": true, "source": "NewBeeBox desktop auth-store", "channel": "creator"}, nil
	}
	if strings.HasPrefix(command.endpoint, "metadata:") {
		value, err := c.http.JSON(ctx, http.MethodGet, c.metadata, nil, nil)
		if err != nil {
			return nil, err
		}
		key := strings.TrimPrefix(command.endpoint, "metadata:")
		rows, ok := value[key].([]any)
		if !ok {
			return nil, failure("publish.newbee_metadata", "metadata omitted selected options")
		}
		return map[string]any{"items": rows, "total": len(rows)}, nil
	}
	if strings.HasPrefix(action, "backup ") {
		value, err := c.backupPost(ctx, "/v3/share/list", map[string]any{})
		if err != nil {
			return nil, err
		}
		rows := newBeeRows(value)
		if rows == nil {
			return nil, failure("publish.newbee_backup", "malformed backup list")
		}
		// The platform can group backups in nested arrays. Keep every group;
		// deletion recovery also relies on this list to prove target absence.
		flat := []any{}
		var collect func([]any) error
		collect = func(items []any) error {
			for _, item := range items {
				switch v := item.(type) {
				case []any:
					if err := collect(v); err != nil {
						return err
					}
				case map[string]any:
					flat = append(flat, v)
				default:
					return failure("publish.newbee_backup", "malformed backup list item")
				}
			}
			return nil
		}
		if err := collect(rows); err != nil {
			return nil, err
		}
		rows = flat
		if action == "backup get" {
			for _, row := range rows {
				obj := newBeeObject(row)
				if newBeeID(newBeePick(obj, "t_id", "id")) == Integer(input, "id") {
					return obj, nil
				}
			}
			return nil, failure("publish.newbee_not_found", "selected backup is absent")
		}
		return map[string]any{"items": rows, "total": len(rows)}, nil
	}
	return c.post(ctx, command.endpoint, newBeeBody(command, input))
}
func newBeeID(v any) int64 {
	switch x := v.(type) {
	case json.Number:
		n, _ := x.Int64()
		return n
	case int:
		return int64(x)
	case int64:
		return x
	case string:
		n, _ := strconv.ParseInt(x, 10, 64)
		return n
	}
	return 0
}
func newBeeEqual(a, b any) bool {
	left, _ := json.Marshal(newBeeDecoded(a))
	right, _ := json.Marshal(newBeeDecoded(b))
	return string(left) == string(right)
}

func (c *newBeeCreator) run(ctx context.Context, action string, command newBeeCreatorCommand, input map[string]any, request Request) (Result, error) {
	result := Result{Platform: "newbee", Action: "creator " + action}
	if action == "backup upload" {
		return c.runBackupUpload(ctx, input, request)
	}
	if action == "backup delete-preview" {
		value, err := c.backupDeletePreview(ctx, input)
		result.Data = value
		result.Complete = err == nil
		return result, err
	}
	if action == "backup delete" {
		return c.runBackupDelete(ctx, input, request)
	}
	if !command.write {
		value, err := c.read(ctx, action, input)
		result.Data = newBeePublic(value)
		result.Complete = err == nil
		return result, err
	}
	// Hold the selected file through fingerprinting, preparation and upload so
	// the persisted intent and submitted bytes describe the same artifact.
	if path := String(input, "file"); path != "" {
		artifact, err := openArtifact(path)
		if err != nil {
			return result, invalid("$.file", "cannot lock selected file")
		}
		defer artifact.Close()
	}
	session, err := c.sessionIdentity(ctx)
	if err != nil {
		return result, err
	}
	fingerprint, err := newBeeFingerprint(ctx, result.Action, input)
	if err != nil {
		return result, err
	}
	requestHash := sha256.Sum256([]byte(request.RequestID))
	operation := hex.EncodeToString(requestHash[:])
	result.OperationID = operation
	receiptPath := filepath.Join(request.Home, "publish", "newbee", operation+".json")
	lease, err := vault.AcquireLease(ctx, filepath.Join(request.Home, "publish", "locks"), "newbee/"+operation)
	if err != nil {
		return result, err
	}
	defer lease.Close()
	if raw, e := os.ReadFile(receiptPath); e == nil {
		var receipt newBeeReceipt
		if newBeeDecodeStored(raw, &receipt) != nil {
			return result, failure("publish.newbee_receipt", "invalid original operation receipt")
		}
		if receipt.Fingerprint != fingerprint {
			return result, invalid("$request", "request ID is already bound to different input")
		}
		if err := newBeeCheckSession(receipt.Session, session); err != nil {
			return result, err
		}
		if receipt.State == "complete" {
			return receipt.Result, nil
		}
		if receipt.State == "rejected" {
			return receipt.Result, failure("publish.newbee_business", "original Creator operation was rejected; it will not be replayed")
		}
		result.Submission = "uncertain"
		result.Data = map[string]any{"replayed": false, "next": "read original resource and reconcile its state"}
		return result, &Error{Code: "publish.newbee_pending", Message: "original Creator operation may have submitted; read the original resource", Exit: 6}
	} else if !os.IsNotExist(e) {
		return result, e
	}
	// Resolve the existing full edit form and dynamic options before recording submission.
	body, err := c.prepare(ctx, action, command, input)
	if err != nil {
		return result, err
	}
	var expected map[string]string
	if strings.HasSuffix(action, " edit") || strings.HasSuffix(action, " create") {
		expected = newBeeExpectedFields(body)
	}
	save := func(state string) error {
		raw, err := json.Marshal(newBeeReceipt{Session: session, Fingerprint: fingerprint, State: state, Result: result, Target: newBeeTarget(input), Expected: expected})
		if err != nil {
			return err
		}
		if err = os.MkdirAll(filepath.Dir(receiptPath), 0700); err != nil {
			return err
		}
		return vault.ReplaceFile(ctx, receiptPath, raw)
	}
	result.Submission = "uncertain"
	if err = save("submitted"); err != nil {
		return result, err
	}
	var value any
	switch {
	case action == "wa attachment-upload":
		value, err = c.uploadAttachment(ctx, String(input, "file"))
	case strings.HasSuffix(action, "media-upload"):
		value, err = c.upload(ctx, command.endpoint, String(input, "file"), nil)
	case action == "plugin upload":
		fields := map[string]string{}
		for k, v := range body {
			if k == "file" {
				continue
			}
			if s, ok := v.(string); ok {
				fields[k] = s
			} else {
				raw, _ := json.Marshal(v)
				fields[k] = string(raw)
			}
		}
		value, err = c.upload(ctx, command.endpoint, String(input, "file"), fields)
	case action == "backup rename":
		value, err = c.backupPost(ctx, command.endpoint, body)
	default:
		value, err = c.post(ctx, command.endpoint, body)
	}
	if err != nil {
		if newBeeRejected(err) {
			result.Submission = "rejected"
			if saveErr := save("rejected"); saveErr != nil {
				return result, saveErr
			}
		}
		return result, err
	}
	result.Submission = "accepted"
	result.Data = newBeePublic(value)
	if err = save("accepted"); err != nil {
		return result, err
	}
	readback, err := c.verify(ctx, action, input, body, value, expected)
	if err != nil {
		return result, err
	}
	result.Data = map[string]any{"receipt": newBeePublic(value), "readback": newBeePublic(readback)}
	result.Complete = true
	result.Verification = "metadata-readback"
	if strings.HasSuffix(action, "media-upload") {
		result.Verification = "upload-receipt"
	}
	if err = save("complete"); err != nil {
		result.Complete = false
		return result, err
	}
	return result, nil
}

func newBeeFieldAliases(resource, key string) []string {
	aliases := []string{key, "t_" + key}
	special := map[string]map[string][]string{
		"plugin": {"mod_categories": {"category_ids", "mod_categories"}, "content_origin": {"t_original", "content_origin"}, "description": {"t_description_v2", "description"}, "intro": {"t_description", "intro"}, "share_state": {"t_share", "share_state"}},
		"config": {"cloud_id": {"t_cloudblackid", "cloud_id"}, "pic_url": {"pic_url", "picture_urls", "piclist"}, "roleid": {"t_roleid", "roleid", "role_id"}},
		"wa":     {"category_id_list": {"category_id_list", "category_ids", "category_list"}},
	}
	if v, ok := special[resource][key]; ok {
		return v
	}
	return aliases
}
func newBeeForm(resource string, detail map[string]any, id int64) map[string]any {
	form := map[string]any{"id": id}
	if resource == "config" {
		delete(form, "id")
		form["tid"] = id
	}
	spec := newBeeCreatorCommands[resource+" edit"].spec
	for key := range spec.Properties {
		if key == "id" || key == "public" {
			continue
		}
		target := key
		if key == "picture_urls" {
			target = "pic_url"
		}
		if v := newBeePick(detail, newBeeFieldAliases(resource, target)...); v != nil {
			form[target] = newBeeDecoded(v)
		}
	}
	share := "share_state"
	if resource == "config" {
		share = "sharing"
	}
	if v := newBeePick(detail, newBeeFieldAliases(resource, share)...); v != nil {
		form[share] = v
	}
	if resource == "config" {
		if v := newBeePick(detail, "t_selected_eml", "selected_eml"); v != nil {
			form["selected_eml"] = newBeeDecoded(v)
		}
	}
	// Retain complete attachment and linked-mod objects, including unknown server fields.
	if resource == "wa" {
		form["wa_log"] = ""
	}
	return form
}
func newBeeValidateRelationships(action string, input map[string]any) error {
	_, verb, _ := strings.Cut(action, " ")
	if strings.HasSuffix(verb, "-set") && verb != "share-code-set" {
		field := "co_authors"
		if strings.HasPrefix(verb, "reference") {
			field = "references"
		}
		seen := map[string]bool{}
		total := float64(0)
		for _, raw := range newBeeRows(input[field]) {
			row := newBeeObject(raw)
			key := fmt.Sprint(row["type"]) + "/" + fmt.Sprint(row["id"])
			if field == "co_authors" {
				key = fmt.Sprint(row["user_id"])
				n, _ := row["share_percent"].(json.Number)
				value, _ := n.Float64()
				total += value
			}
			if seen[key] {
				return invalid("$."+field, "duplicate relationship target")
			}
			seen[key] = true
		}
		if total > 100 {
			return invalid("$.co_authors", "combined shares exceed 100 percent")
		}
	}
	return nil
}

func (c *newBeeCreator) prepare(ctx context.Context, action string, command newBeeCreatorCommand, input map[string]any) (map[string]any, error) {
	resource, verb, _ := strings.Cut(action, " ")
	body := newBeeBody(command, input)

	if verb == "delete" {
		if !Bool(input, "confirm_delete") {
			return nil, invalid("$.confirm_delete", "must be true")
		}
		value, err := c.read(ctx, resource+" get", map[string]any{"id": input["id"]})
		if err != nil {
			return nil, err
		}
		if newBeeID(newBeePick(newBeeObject(value), "id", "t_id")) != Integer(input, "id") {
			return nil, failure("publish.newbee_identity", "delete target identity differs from selection")
		}
	}
	if verb == "edit" {
		raw, err := c.read(ctx, resource+" get", map[string]any{"id": input["id"]})
		if err != nil {
			return nil, err
		}
		detail := newBeeObject(raw)
		if detail == nil {
			return nil, failure("publish.newbee_detail", "selected resource detail is missing")
		}
		if got := newBeeID(newBeePick(detail, "id", "t_id")); got != Integer(input, "id") {
			return nil, failure("publish.newbee_identity", "detail belongs to a different resource")
		}
		body = newBeeForm(resource, detail, Integer(input, "id"))
		for key, value := range input {
			if key == "id" || key == "public" {
				continue
			}
			if key == "picture_urls" {
				key = "pic_url"
			}
			body[key] = value
		}
	}
	if verb == "create" || verb == "edit" {
		if resource == "config" {
			if v, ok := body["picture_urls"]; ok {
				body["pic_url"] = v
				delete(body, "picture_urls")
			}
		}
		if v, ok := input["public"].(bool); ok {
			share := 0
			if v {
				share = 1
			}
			key := "share_state"
			if resource == "config" {
				key = "sharing"
			} else if resource == "wa" && !v {
				share = 2
			}
			body[key] = share
			delete(body, "public")
		}
		if resource == "plugin" && verb == "create" {
			body["share_state"] = 0
			body["link_to_channel"] = false
		}
		shareKey := "share_state"
		if resource == "config" {
			shareKey = "sharing"
		}
		if newBeeID(body[shareKey]) != 1 {
			body["link_to_channel"] = false
		}
		newBeeNormalizePrice(body)
		if err := c.validateOptions(ctx, resource, body, input); err != nil {
			return nil, err
		}
		if err := c.validateResource(ctx, resource, body, input); err != nil {
			return nil, err
		}
	}
	if action == "plugin upload" {
		current, err := c.read(ctx, "plugin get", map[string]any{"id": input["id"]})
		if err != nil {
			return nil, err
		}
		detail := newBeeObject(current)
		if newBeeID(newBeePick(detail, "id", "t_id")) != Integer(input, "id") {
			return nil, failure("publish.newbee_identity", "upload target identity differs from selection")
		}
		if _, provided := input["link_to_channel"]; !provided {
			body["link_to_channel"] = newBeePick(detail, "t_link_to_channel", "link_to_channel") == true
		}
		raw, err := c.read(ctx, "plugin versions", map[string]any{"id": input["id"]})
		if err != nil {
			return nil, err
		}
		for _, row := range newBeeRows(raw) {
			v := newBeeObject(row)
			if strings.EqualFold(fmt.Sprint(newBeePick(v, "t_display_name", "display_name", "version", "t_version")), String(input, "version")) {
				return nil, invalid("$.version", "version already exists")
			}
		}
		body["mod_id"] = input["id"]
		delete(body, "id")
		delete(body, "file")
		if err := c.validateBuilds(ctx, input["game_version_list"]); err != nil {
			return nil, err
		}
	}
	if action == "wa changelog-edit" {
		row, err := c.findWALog(ctx, Integer(input, "wa_id"), Integer(input, "id"))
		if err != nil {
			return nil, err
		}
		if row == nil {
			return nil, invalid("$.id", "changelog does not belong to the selected WA")
		}
	}
	if action == "wa upload" {
		raw, err := c.read(ctx, "wa get", map[string]any{"id": input["id"]})
		if err != nil {
			return nil, err
		}
		detail := newBeeObject(raw)
		if !newBeeVersionGreater(String(input, "version"), fmt.Sprint(newBeePick(detail, "version", "t_version"))) {
			return nil, invalid("$.version", "version must be greater than the current version")
		}
		for _, key := range []string{"wa_str_titles", "link_to_channel"} {
			if _, ok := body[key]; !ok {
				if v := newBeePick(detail, key, "t_"+key); v != nil {
					body[key] = newBeeDecoded(v)
				}
			}
		}
	}
	return body, nil
}
func newBeeVersionGreater(a, b string) bool {
	parse := func(v string) ([]int, error) {
		parts := strings.Split(v, ".")
		if len(parts) != 3 {
			return nil, fmt.Errorf("invalid version")
		}
		out := []int{}
		for _, p := range parts {
			n, err := strconv.Atoi(p)
			if err != nil || n < 0 {
				return nil, fmt.Errorf("invalid version")
			}
			out = append(out, n)
		}
		return out, nil
	}
	left, err := parse(a)
	if err != nil {
		return false
	}
	right, err := parse(b)
	if err != nil {
		return false
	}
	for i := range left {
		if left[i] != right[i] {
			return left[i] > right[i]
		}
	}
	return false
}
func (c *newBeeCreator) validateOptions(ctx context.Context, resource string, body, input map[string]any) error {
	for field, action := range map[string]string{"content_origin": "options content-origins", "subscribe_plan_level": "options subscribe-plans", "time_range": "options time-ranges"} {
		if _, changed := input[field]; !changed {
			continue
		}
		if fmt.Sprint(body[field]) == "" {
			continue
		}
		value, err := c.read(ctx, action, map[string]any{})
		if err != nil {
			return err
		}
		found := false
		for _, row := range newBeeRows(value) {
			obj := newBeeObject(row)
			if newBeeEqual(obj["value"], body[field]) {
				found = true
			}
		}
		if !found {
			return invalid("$."+field, "value is absent from current platform options")
		}
	}
	return nil
}

func (c *newBeeCreator) verify(ctx context.Context, action string, input, body map[string]any, receipt any, expected map[string]string) (any, error) {
	pending := func() (any, error) {
		return nil, &Error{Code: "publish.newbee_verification", Message: "submitted operation differs from resource readback; retain original request", Exit: 6}
	}
	resource, verb, _ := strings.Cut(action, " ")
	if verb == "delete" {
		return c.verifyDeletedResource(ctx, resource, Integer(input, "id"))
	}
	if action == "wa attachment-upload" {
		if receipt == nil {
			return pending()
		}
		return receipt, nil
	}
	if action == "wa share-code-set" {
		value, err := c.read(ctx, "wa get", map[string]any{"id": input["id"]})
		if err != nil {
			return nil, err
		}
		obj := newBeeObject(value)
		code := newBeePick(obj, "share_code", "t_share_code", "shareCode")
		if code == nil || fmt.Sprint(code) == "" {
			return pending()
		}
		return value, nil
	}
	if action == "wa changelog-edit" {
		row, err := c.findWALog(ctx, Integer(input, "wa_id"), Integer(input, "id"))
		if err != nil {
			return nil, err
		}
		if row == nil || !newBeeEqual(newBeePick(row, "content", "wa_log", "t_content"), input["wa_log"]) {
			return pending()
		}
		return row, nil
	}

	if verb == "media-upload" {
		if receipt == nil {
			return pending()
		}
		return receipt, nil
	}
	if strings.HasSuffix(verb, "-set") {
		return c.verifyRelation(ctx, action, input, body)
	}
	if action == "backup rename" {
		v, err := c.read(ctx, "backup get", map[string]any{"id": input["id"]})
		if err != nil {
			return nil, err
		}
		if !newBeeEqual(newBeePick(newBeeObject(v), "t_name", "name"), input["name"]) {
			return pending()
		}
		return v, nil
	}
	if action == "plugin changelog-edit" {
		v, err := c.read(ctx, "plugin changelog-get", map[string]any{"id": input["id"]})
		if err != nil {
			return nil, err
		}
		detail := newBeeObject(v)
		actual, present := detail["changelog"]
		if newBeeID(detail["file_id"]) != Integer(input, "id") || !present {
			return pending()
		}
		if !newBeeEqual(actual, input["changelog"]) && !(actual == nil && String(input, "changelog") == "") {
			return pending()
		}
		return v, nil
	}
	if action == "plugin upload" {
		v, err := c.read(ctx, "plugin versions", map[string]any{"id": input["id"]})
		if err != nil {
			return nil, err
		}
		for _, row := range newBeeRows(v) {
			obj := newBeeObject(row)
			if fmt.Sprint(newBeePick(obj, "t_display_name", "display_name", "version", "t_version")) == String(input, "version") {
				bindings := map[string]bool{}
				for _, bound := range newBeeRows(newBeePick(obj, "game_version_list", "t_game_version_list", "versions")) {
					if row := newBeeObject(bound); row != nil {
						bound = newBeePick(row, "version", "build", "support_version", "value")
					}
					bindings[fmt.Sprint(bound)] = true
				}
				for _, wanted := range newBeeRows(input["game_version_list"]) {
					if !bindings[fmt.Sprint(wanted)] {
						return pending()
					}
				}
				if len(bindings) == 0 {
					return pending()
				}
				return row, nil
			}
		}
		return pending()
	}
	if action == "wa upload" {
		v, err := c.read(ctx, "wa changelog-latest", map[string]any{"id": input["id"]})
		if err != nil {
			return nil, err
		}
		if fmt.Sprint(newBeePick(newBeeObject(v), "version", "t_version")) != String(input, "version") {
			return pending()
		}
		return v, nil
	}
	id := Integer(input, "id")
	if verb == "create" {
		id = newBeeID(newBeePick(newBeeObject(receipt), "id", "t_id", "mod_id", "wa_id"))
		if id == 0 {
			return pending()
		}
	}
	raw, err := c.read(ctx, resource+" get", map[string]any{"id": json.Number(strconv.FormatInt(id, 10))})
	if err != nil {
		return nil, err
	}
	detail := newBeeObject(raw)
	if newBeeID(newBeePick(detail, "id", "t_id")) != id {
		return pending()
	}
	if expected == nil {
		expected = newBeeExpectedFields(body)
	}
	for key, want := range expected {
		got := newBeePick(detail, newBeeFieldAliases(resource, key)...)
		if want != newBeeFieldDigest(got) {
			return pending()
		}
	}
	return detail, nil
}

// Persist the prepared form's expectations without storing WA/config bodies.
// Recovery must check the original canonical and retained fields, not rebuild
// them from today's resource or the caller's partial edit.
func newBeeExpectedFields(body map[string]any) map[string]string {
	expected := map[string]string{}
	for key, value := range body {
		if key == "id" || key == "tid" || key == "wa_log" || key == "wa_str" || key == "string_mode" {
			continue
		}
		expected[key] = newBeeFieldDigest(value)
	}
	return expected
}

func newBeeFieldDigest(value any) string {
	raw, _ := json.Marshal(newBeeDecoded(value))
	sum := sha256.Sum256(raw)
	return hex.EncodeToString(sum[:])
}
func (c *newBeeCreator) verifyRelation(ctx context.Context, action string, input, body map[string]any) (any, error) {
	readAction := strings.TrimSuffix(action, "-set") + "-list"
	value, err := c.read(ctx, readAction, map[string]any{"id": input["id"]})
	if err != nil {
		return nil, err
	}
	field := "co_authors"
	if strings.Contains(action, "reference") {
		field = "references"
		for key, want := range map[string]any{"source_type": body["source_type"], "content_type": body["source_type"], "source_id": input["id"], "content_id": input["id"]} {
			if got, exists := newBeeObject(value)[key]; exists && want != nil && !newBeeEqual(got, want) {
				return nil, &Error{Code: "publish.newbee_verification", Message: "relationship readback belongs to another source", Exit: 6}
			}
		}
	}
	rows := newBeeRows(value)
	if object := newBeeObject(value); rows == nil && object != nil {
		rows = newBeeRows(object[field])
	}
	expected := newBeeRows(input[field])
	observed := map[string]bool{}
	wanted := map[string]bool{}
	for _, row := range expected {
		obj := newBeeObject(row)
		if field == "co_authors" {
			wanted[fmt.Sprint(obj["user_id"])+"/"+fmt.Sprint(obj["share_percent"])] = true
		} else {
			wanted[fmt.Sprint(obj["type"])+"/"+fmt.Sprint(obj["id"])] = true
		}
	}
	for _, row := range rows {
		obj := newBeeObject(row)
		if field == "co_authors" {
			observed[fmt.Sprint(newBeePick(obj, "user_id", "t_user_id", "id"))+"/"+fmt.Sprint(newBeePick(obj, "share_percent", "t_share_percent", "ratio"))] = true
		} else {
			kind := newBeeID(newBeePick(obj, "type", "content_type", "t_type"))
			if kind < 2 || kind > 4 {
				return nil, &Error{Code: "publish.newbee_verification", Message: "reference target type is missing or invalid", Exit: 6}
			}
			observed[strconv.FormatInt(kind-1, 10)+"/"+fmt.Sprint(newBeePick(obj, "id", "content_id", "t_id"))] = true
		}
	}
	if rows == nil || len(rows) != len(expected) || !reflect.DeepEqual(wanted, observed) {
		return nil, &Error{Code: "publish.newbee_verification", Message: "relationship replacement differs from readback", Exit: 6}
	}
	return value, nil
}

// The edit endpoint accepts a log ID alone. Resolve it inside the selected WA
// before submitting, and use the same paged lookup for original-request readback.
func (c *newBeeCreator) findWALog(ctx context.Context, waID, logID int64) (map[string]any, error) {
	seen := map[int64]bool{}
	for page := int64(1); page <= 1000; page++ {
		value, err := c.read(ctx, "wa changelog-list", map[string]any{"id": json.Number(fmt.Sprint(waID)), "page": json.Number(fmt.Sprint(page)), "page_size": json.Number("100")})
		if err != nil {
			return nil, err
		}
		rows := newBeeRows(value)
		if rows == nil || len(rows) > 100 {
			return nil, failure("publish.newbee_changelog", "invalid WA changelog page")
		}
		for _, raw := range rows {
			row := newBeeObject(raw)
			id := newBeeID(newBeePick(row, "id", "t_id", "wa_log_id"))
			if id <= 0 || seen[id] {
				return nil, failure("publish.newbee_changelog", "WA changelog IDs repeat or are missing")
			}
			seen[id] = true
			if id == logID {
				return row, nil
			}
		}
		if len(rows) < 100 {
			return nil, nil
		}
	}
	return nil, failure("publish.newbee_changelog", "WA changelog lookup reached its pagination bound")
}
