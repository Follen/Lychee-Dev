package publish

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/follenfang/lycheedev/internal/vault"
)

func modusBackupActions() []string {
	return []string{"backup scan", "backup generate", "backup create", "backup upload", "backup status", "backup resume", "backup verify", "backup rename", "backup delete-preview", "backup delete"}
}
func modusBackupMutates(action string) bool {
	return action == "backup generate" || action == "backup create" || action == "backup upload" || action == "backup resume" || action == "backup rename" || action == "backup delete"
}
func modusBackupInput(action string) (InputSpec, bool) {
	known := false
	for _, a := range modusBackupActions() {
		known = known || action == a
	}
	if !known {
		return InputSpec{}, false
	}
	id := Field{Type: "integer", Minimum: Int(1)}
	str := Field{Type: "string", Nonempty: true}
	build := Field{Type: "integer", Minimum: Int(0), Maximum: Int(4)}
	s := InputSpec{Properties: map[string]Field{"server_type": build}, Required: []string{"server_type"}, Example: map[string]any{"server_type": 0}, Effects: []string{"uses current ModUs main account and explicit Build; preserves original operation and generated artifacts"}}
	switch action {
	case "backup status", "backup resume", "backup verify":
		s.Properties = map[string]Field{"request": {Type: "string", Nonempty: true, MaxLength: 128}}
		s.Required = []string{"request"}
		s.Example = map[string]any{"request": "backup-1"}
		if action == "backup verify" {
			s.Properties["backup_id"] = id
		}
		if action == "backup status" {
			s.Effects = []string{"reads original local backup operation only"}
		}
	case "backup scan":
		s.Properties["game_root"] = str
		s.Required = append(s.Required, "game_root")
		s.Example["game_root"] = "D:/Games/WoW/_retail_"
		s.Effects = []string{"lists local names and characters only; no credentials or writes"}
	case "backup generate", "backup create":
		s.Properties["game_root"] = str
		s.Properties["output_dir"] = str
		s.Properties["backup_name"] = Field{Type: "string", Nonempty: true, MaxLength: 80}
		for _, k := range []string{"addon_names", "font_names", "material_names"} {
			s.Properties[k] = Field{Type: "array", Items: &str, Description: "Explicit unique single names; no wildcard"}
		}
		s.Properties["roles"] = Field{Type: "array", Items: &Field{Type: "object", Properties: map[string]Field{"account": str, "server": str, "role": str}, Required: []string{"account", "server", "role"}}, Description: "Exact roles plus selected account common settings; other characters excluded"}
		s.Required = append(s.Required, "game_root", "output_dir", "backup_name")
		s.Example["game_root"] = "D:/Games/WoW/_retail_"
		s.Example["output_dir"] = "D:/Backups/Example"
		s.Example["backup_name"] = "Example"
		s.Example["addon_names"] = []any{"Example"}
		if action == "backup generate" {
			s.Effects = []string{"copies only selected local files to a new directory and writes native config-backup.v2 manifest; no credentials"}
		}
	case "backup upload":
		s.Properties["manifest_file"] = str
		s.Required = append(s.Required, "manifest_file")
		s.Example["manifest_file"] = "D:/Backups/Example/manifest.v2.json"
	case "backup rename":
		s.Properties["backup_id"] = id
		s.Properties["backup_name"] = Field{Type: "string", Nonempty: true, MaxLength: 80}
		s.Required = append(s.Required, "backup_id", "backup_name")
		s.Example["backup_id"] = 1
		s.Example["backup_name"] = "Renamed"
	case "backup delete-preview":
		s.Properties["backup_ids"] = Field{Type: "array", Nonempty: true, Items: &id}
		for _, k := range []string{"name_contains", "name_equals", "created_before", "created_after"} {
			s.Properties[k] = str
		}
		s.Example["backup_ids"] = []any{1}
		s.Effects = []string{"reads selected backups and complete configuration references in all five Builds; no deletion"}
	case "backup delete":
		s.Properties["backup_ids"] = Field{Type: "array", Nonempty: true, Items: &id}
		s.Properties["confirm_delete"] = Field{Type: "boolean"}
		s.Required = append(s.Required, "backup_ids", "confirm_delete")
		s.Example["backup_ids"] = []any{1}
		s.Example["confirm_delete"] = true
	}
	return s, true
}

type modusBackupObject struct {
	Name        string   `json:"name"`
	File        string   `json:"file"`
	Type        int      `json:"fileType"`
	ContentType string   `json:"contentType"`
	SHA256      string   `json:"sha256"`
	MD5         string   `json:"md5"`
	Size        int64    `json:"size"`
	Key         string   `json:"key,omitempty"`
	Phase       string   `json:"phase"`
	Section     string   `json:"section,omitempty"`
	Account     string   `json:"account,omitempty"`
	Roles       []string `json:"roles,omitempty"`
}
type modusBackupDeleteItem struct {
	ID         string           `json:"id"`
	Phase      string           `json:"phase"`
	Protected  bool             `json:"protected"`
	References []map[string]any `json:"references,omitempty"`
}
type modusBackupState struct {
	Schema       string                  `json:"schema"`
	ID           string                  `json:"id"`
	Action       string                  `json:"action"`
	Account      string                  `json:"account"`
	Build        int64                   `json:"build"`
	Input        map[string]any          `json:"input"`
	InputDigest  string                  `json:"inputDigest"`
	Phase        string                  `json:"phase"`
	Manifest     string                  `json:"manifest,omitempty"`
	ManifestSHA  string                  `json:"manifestSha256,omitempty"`
	ManifestData map[string]any          `json:"manifestData,omitempty"`
	Objects      []modusBackupObject     `json:"objects,omitempty"`
	Body         map[string]any          `json:"body,omitempty"`
	Before       []string                `json:"before,omitempty"`
	CloudID      string                  `json:"cloudId,omitempty"`
	Items        []modusBackupDeleteItem `json:"items,omitempty"`
	Complete     bool                    `json:"complete"`
}

func modusBackupStatePath(home, id string) (string, error) {
	if !validRequest(id) {
		return "", invalid("$.request", "request must contain 1-128 ASCII letters, digits, underscores or hyphens")
	}
	return filepath.Join(home, "publish", "modus", "backups", id, "state.json"), nil
}
func saveModusBackup(ctx context.Context, home string, s *modusBackupState) error {
	p, err := modusBackupStatePath(home, s.ID)
	if err != nil {
		return err
	}
	raw, err := json.Marshal(s)
	if err != nil {
		return err
	}
	return vault.ReplaceFile(ctx, p, raw)
}

func modusBackupWriteFailure(ctx context.Context, home string, s *modusBackupState, err error) error {
	if modusRejected(err) {
		s.Phase = "rejected"
		if saveErr := saveModusBackup(ctx, home, s); saveErr != nil {
			return saveErr
		}
	}
	return err
}
func readModusBackup(home, id string) (modusBackupState, error) {
	var s modusBackupState
	p, err := modusBackupStatePath(home, id)
	if err != nil {
		return s, err
	}
	raw, err := modusReadBounded(p, 8<<20)
	if err != nil {
		return s, err
	}
	d := json.NewDecoder(bytes.NewReader(raw))
	d.UseNumber()
	d.DisallowUnknownFields()
	if d.Decode(&s) != nil || d.Decode(new(any)) != io.EOF || s.Schema != "lycheedev.publish.modus-backup.v1" || s.ID != id || s.InputDigest != modusInputDigest(s.Input) || s.Build < 0 || s.Build > 4 {
		return s, failure("publish.modus_backup_state", "invalid original backup state")
	}
	return s, nil
}
func (s *modusBackupState) result(action string) Result {
	complete := s.Complete
	for _, item := range s.Items {
		if item.Protected {
			complete = false
		}
	}
	submission := "not_submitted"
	for _, o := range s.Objects {
		if o.Phase == "put-done" || o.Phase == "stored" {
			submission = "submitted"
		}
	}
	for _, item := range s.Items {
		if item.Phase == "complete" {
			submission = "submitted"
		}
	}
	if s.Phase == "readback" || s.Phase == "link" || s.Phase == "rename-readback" || s.Complete {
		submission = "submitted"
	}
	for _, o := range s.Objects {
		if strings.Contains(o.Phase, "intent") {
			submission = "uncertain"
		}
	}
	for _, item := range s.Items {
		if strings.Contains(item.Phase, "intent") {
			submission = "uncertain"
		}
	}
	if strings.Contains(s.Phase, "intent") {
		submission = "uncertain"
	}
	if s.Phase == "rejected" {
		submission = "rejected"
	}
	data := map[string]any{"originalAction": s.Action, "phase": s.Phase, "backup_id": s.CloudID, "manifest_file": s.Manifest, "objects": len(s.Objects), "items": s.Items, "replayAllowed": false, "nextCommand": "publish modus backup resume"}
	if s.Complete || s.Phase == "rejected" {
		data["nextCommand"] = ""
	} else if submission == "uncertain" {
		data["nextCommand"] = "publish modus backup verify"
	}
	return Result{Platform: "modus", Action: action, Complete: complete, OperationID: s.ID, Submission: submission, Data: data}
}
func modusBackupPending(message string) error {
	return &Error{Code: "publish.modus_backup_pending", Message: message, Exit: 6}
}
func runModusBackup(ctx context.Context, action string, r Request, in map[string]any, c *modusClient) (Result, error) {
	result := Result{Platform: "modus", Action: action}
	if err := ctx.Err(); err != nil {
		return result, err
	}
	if action == "backup delete" && !Bool(in, "confirm_delete") {
		return result, invalid("$.confirm_delete", "must be true")
	}
	if values, ok := in["backup_ids"].([]any); ok {
		seen := map[string]bool{}
		for _, v := range values {
			id, err := modusBackupID(v)
			if err != nil {
				return result, err
			}
			if seen[id] {
				return result, invalid("$.backup_ids", "duplicate backup ID")
			}
			seen[id] = true
		}
	}
	if action == "backup delete-preview" {
		_, hasIDs := in["backup_ids"]
		if !hasIDs && String(in, "name_contains") == "" && String(in, "name_equals") == "" && String(in, "created_before") == "" && String(in, "created_after") == "" {
			return result, invalid("$", "select explicit IDs or name/date filters")
		}
		for _, key := range []string{"created_before", "created_after"} {
			if value := String(in, key); value != "" {
				if _, err := time.Parse(time.RFC3339, value); err != nil {
					return result, invalid("$."+key, "use an ISO 8601 timestamp with timezone")
				}
			}
		}
	}
	if action == "backup status" {
		s, err := readModusBackup(r.Home, String(in, "request"))
		return s.result(action), err
	}
	if action == "backup scan" {
		v, err := scanModusBackup(ctx, in)
		result.Data = v
		result.Complete = err == nil
		return result, err
	}
	if action == "backup generate" {
		v, err := generateModusBackup(ctx, in, r.Plan)
		result.Data = v
		result.Complete = err == nil
		return result, err
	}
	if r.Plan {
		var data any = map[string]any{"plan": true, "remoteChecked": false}
		var err error
		if action == "backup create" {
			data, err = generateModusBackup(ctx, in, true)
		} else if action == "backup upload" {
			_, _, err = loadModusBackupManifest(ctx, String(in, "manifest_file"), Integer(in, "server_type"))
			data = map[string]any{"plan": true, "manifestChecked": err == nil, "remoteChecked": false}
		}
		result.Complete = err == nil
		result.Data = data
		return result, err
	}
	if c == nil || !c.main {
		return result, capability("publish.modus_backup_session", "backup operations require the explicit current main-client account")
	}
	if action == "backup delete-preview" {
		v, err := previewModusBackup(ctx, c, in)
		result.Data = v
		result.Complete = err == nil
		return result, err
	}
	id := r.RequestID
	if action == "backup resume" || action == "backup verify" {
		id = String(in, "request")
	}
	p, err := modusBackupStatePath(r.Home, id)
	if err != nil {
		return result, err
	}
	lease, err := vault.TryAcquireLease(ctx, filepath.Join(r.Home, "publish", "modus", "backup-locks"), id)
	if err != nil {
		return result, err
	}
	defer lease.Close()
	s, err := readModusBackup(r.Home, id)
	if err != nil && !errors.Is(err, os.ErrNotExist) {
		return result, err
	}
	account, errAccount := c.account(ctx)
	if errAccount != nil {
		return result, errAccount
	}
	if err == nil {
		if s.Account != account {
			return s.result(action), failure("publish.modus_backup_account", "current account differs from original backup operation")
		}
		if action != "backup resume" && action != "backup verify" && (s.Action != action || s.InputDigest != modusInputDigest(in)) {
			return result, invalid("$.request", "request belongs to different original input")
		}
	} else {
		if action == "backup resume" || action == "backup verify" {
			return result, err
		}
		s = modusBackupState{Schema: "lycheedev.publish.modus-backup.v1", ID: id, Action: action, Account: account, Build: Integer(in, "server_type"), Input: in, InputDigest: modusInputDigest(in), Phase: "prepare"}
		if err = saveModusBackup(ctx, r.Home, &s); err != nil {
			return result, err
		}
	}
	if s.Complete {
		return s.result(action), nil
	}
	if action == "backup verify" && in["backup_id"] != nil {
		remote := fmtModusID(in["backup_id"])
		if s.CloudID != "" && s.CloudID != remote {
			return s.result(action), invalid("$.backup_id", "differs from retained original cloud identity")
		}
		s.CloudID = remote
	}
	if s.Action == "backup rename" || s.Action == "backup delete" {
		err = continueModusBackupChange(ctx, c, r.Home, &s, action == "backup verify")
	} else {
		err = continueModusBackupUpload(ctx, c, r.Home, filepath.Dir(p), &s, action == "backup verify")
	}
	return s.result(action), err
}
