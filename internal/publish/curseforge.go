package publish

import (
	"archive/zip"
	"bytes"
	"context"
	"crypto/sha1"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"os"
	"path"
	"path/filepath"
	"strconv"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/follenfang/lycheedev/internal/vault"
)

var cfConfigKeys = []string{"CURSEFORGE_AUTHOR_ID", "CURSEFORGE_API_KEY", "CURSEFORGE_UPLOAD_TOKEN"}

func CurseForgeInput(action string) (InputSpec, bool) {
	s := InputSpec{Properties: map[string]Field{}, Required: []string{}, Example: map[string]any{}, Effects: []string{"network read; CurseForge Core query key and Upload token are separate"}}
	id := Field{Type: "integer", Minimum: Int(1), Description: "Explicit positive platform ID"}
	switch action {
	case "session doctor":
		s.Effects = []string{"reads the selected local profile and environment; reports presence only"}
	case "project list":
		s.Properties["author_id"] = id
		s.Example = map[string]any{"author_id": 123}
	case "plugin game-versions":
	case "plugin status", "plugin verify":
		s.Properties = map[string]Field{"project_id": id, "request": {Type: "string", Nonempty: true, MaxLength: 128, Description: "Original upload request ID"}}
		s.Required = []string{"project_id", "request"}
		s.Example = map[string]any{"project_id": 123, "request": "release-1"}
		if action == "plugin verify" {
			s.Properties["file_id"] = id
			s.Required = append(s.Required, "file_id")
			s.Example["file_id"] = 456
		} else {
			s.Effects = []string{"reads the original local upload receipt; no credentials or network"}
		}
	case "plugin upload":
		relation := Field{Type: "object", Properties: map[string]Field{"slug": {Type: "string", Nonempty: true, Description: "Related CurseForge project slug"}, "type": {Type: "string", Enum: []string{"embeddedLibrary", "incompatible", "optionalDependency", "requiredDependency", "tool"}, Description: "Platform dependency type"}, "project_id": id}, Required: []string{"slug", "type"}}
		s.Properties = map[string]Field{
			"project_id": id, "file": {Type: "string", Nonempty: true, Description: "Local plugin ZIP path; original bytes are locked, hashed and uploaded"},
			"changelog": {Type: "string", MaxLength: 1 << 20, Description: "Release changelog"}, "changelog_type": {Type: "string", Enum: []string{"text", "html", "markdown"}, Description: "Changelog format"},
			"display_name": {Type: "string", Nonempty: true, Description: "Optional release display name"}, "release_type": {Type: "string", Enum: []string{"alpha", "beta", "release"}, Description: "Release channel"},
			"game_versions": {Type: "array", Nonempty: true, Items: &id, Description: "IDs returned by plugin game-versions"}, "game_version_names": {Type: "array", Nonempty: true, Items: &Field{Type: "string", Nonempty: true}, Description: "Exact platform version names"},
			"parent_file_id": id, "is_marked_for_manual_release": {Type: "boolean", Description: "Keep the uploaded file for manual release"},
			"relations": {Type: "object", Properties: map[string]Field{"projects": {Type: "array", Items: &relation}}, Required: []string{"projects"}, Description: "Project dependencies"},
		}
		s.Required = []string{"project_id", "file", "changelog", "release_type"}
		s.MutuallyExclusive = [][]string{{"parent_file_id", "game_versions"}, {"parent_file_id", "game_version_names"}}
		s.Example = map[string]any{"project_id": 123, "file": "Addon.zip", "changelog": "Fix an error.", "release_type": "release", "game_versions": []any{12345}}
		s.DynamicOptions = map[string]string{"game_versions": "publish curseforge plugin game-versions", "game_version_names": "publish curseforge plugin game-versions"}
		s.Effects = []string{"reads and locks the local ZIP", "persists intent before one upload", "writes the explicitly selected remote project", "retains receipt; uncertain submission never permits replay"}
	default:
		return InputSpec{}, false
	}
	return s, true
}

type cfClient struct {
	home         string
	http         *HTTP
	core, upload string
	config       map[string]string
}

func newCF(home string) (*cfClient, error) {
	c := &cfClient{home: home, http: NewHTTP(), core: "https://api.curseforge.com", upload: "https://wow.curseforge.com", config: map[string]string{}}
	file := filepath.Join(home, "publish", "curseforge.env")
	f, err := os.Open(file)
	if err == nil {
		defer f.Close()
		raw, e := io.ReadAll(io.LimitReader(f, 65537))
		if e != nil || len(raw) > 65536 || !utf8.Valid(raw) {
			return nil, capability("publish.credentials_read", "invalid CurseForge profile")
		}
		for _, line := range strings.Split(strings.TrimPrefix(string(raw), "\ufeff"), "\n") {
			line = strings.TrimSpace(line)
			if line == "" || strings.HasPrefix(line, "#") {
				continue
			}
			key, value, ok := strings.Cut(line, "=")
			key = strings.TrimSpace(key)
			known := false
			for _, allowed := range cfConfigKeys {
				known = known || key == allowed
			}
			if !ok || !known {
				return nil, invalid("$profile", "expected one of the three CurseForge NAME=VALUE fields")
			}
			if _, found := c.config[key]; found {
				return nil, invalid("$profile", "duplicate field")
			}
			c.config[key] = strings.TrimSpace(value)
		}
	} else if !errors.Is(err, os.ErrNotExist) {
		return nil, capability("publish.credentials_read", "cannot read CurseForge profile")
	}
	for _, key := range cfConfigKeys {
		if value := strings.TrimSpace(os.Getenv(key)); value != "" {
			c.config[key] = value
		}
	}
	return c, nil
}
func (c *cfClient) headers(key, header string) (http.Header, error) {
	value := c.config[key]
	if value == "" {
		return nil, &Error{Code: "publish.credentials_missing", Path: key, Message: "required platform credential is absent", Exit: 3}
	}
	return http.Header{http.CanonicalHeaderKey(header): []string{value}}, nil
}
func RunCurseForge(ctx context.Context, action string, request Request) (Result, error) {
	result := Result{Platform: "curseforge", Action: action}
	spec, ok := CurseForgeInput(action)
	if !ok {
		return result, invalid("$action", "unknown CurseForge action")
	}
	input, err := spec.Decode(request.Input)
	if err != nil {
		return result, err
	}
	if err = ctx.Err(); err != nil {
		return result, err
	}
	if request.Plan {
		result.Complete = true
		plan := map[string]any{"plan": true, "completionScope": "local_plan", "effects": spec.Effects, "remoteChecked": false, "remoteConditions": "execution requires credentials; the platform validates submitted versions and target permissions when handling the request"}
		result.Data = plan
		if action == "plugin upload" {
			artifact, err := inspectArtifact(ctx, String(input, "file"))
			if err != nil {
				return result, err
			}
			defer artifact.file.Close()
			plan["project_id"] = Integer(input, "project_id")
			plan["archive"], plan["sha256"], plan["bytes"] = artifact.name, artifact.sha256, artifact.size
			plan["localChecks"] = []string{"input schema", "ZIP structure and readable archive bytes", "archive hashes"}
			plan["versionSelection"] = "unspecified"
			plan["versionGuidance"] = "Game versions are optional and none were selected. To target a specific client/version, run publish curseforge plugin game-versions and supply game_versions or game_version_names; release_type is the release channel, not the game client."
			if input["parent_file_id"] != nil {
				plan["versionSelection"] = "parent"
				plan["versionGuidance"] = "A parent file is selected; separate game version selection is omitted. The platform validates the parent during upload."
			} else if input["game_versions"] != nil || input["game_version_names"] != nil {
				plan["versionSelection"] = "explicit"
				plan["versionGuidance"] = "Explicit version selection is present; this offline plan does not validate it against the platform catalog."
			}
		}
		return result, nil
	}
	if action == "plugin status" {
		record, err := readCFReceipt(request.Home, Integer(input, "project_id"), String(input, "request"))
		if err != nil {
			return result, err
		}
		return record.result(action), nil
	}
	client, err := newCF(request.Home)
	if err != nil {
		return result, err
	}
	return client.run(ctx, action, input, request.RequestID)
}
func (c *cfClient) run(ctx context.Context, action string, input map[string]any, requestID string) (Result, error) {
	result := Result{Platform: "curseforge", Action: action}
	switch action {
	case "session doctor":
		fields := []any{}
		ready := true
		for _, key := range cfConfigKeys {
			present := c.config[key] != ""
			ready = ready && present
			fields = append(fields, map[string]any{"name": key, "present": present})
		}
		result.Complete = true
		result.Data = map[string]any{"fields": fields, "ready": ready, "configPath": filepath.Join(c.home, "publish", "curseforge.env")}
	case "project list":
		return c.projects(ctx, input)
	case "plugin game-versions":
		headers, err := c.headers("CURSEFORGE_UPLOAD_TOKEN", "X-Api-Token")
		if err != nil {
			return result, err
		}
		value, err := c.http.Value(ctx, http.MethodGet, c.upload+"/api/game/versions", headers, nil)
		if err != nil {
			return result, err
		}
		result.Complete = true
		result.Data = value
	case "plugin upload":
		return c.uploadFile(ctx, input, requestID)
	case "plugin verify":
		return c.verify(ctx, Integer(input, "project_id"), String(input, "request"), Integer(input, "file_id"))
	}
	return result, nil
}
func (c *cfClient) projects(ctx context.Context, input map[string]any) (Result, error) {
	result := Result{Platform: "curseforge", Action: "project list"}
	headers, err := c.headers("CURSEFORGE_API_KEY", "x-api-key")
	if err != nil {
		return result, err
	}
	author := Integer(input, "author_id")
	if author == 0 {
		author, err = strconv.ParseInt(c.config["CURSEFORGE_AUTHOR_ID"], 10, 64)
		if err != nil || author < 1 {
			return result, invalid("$.author_id", "provide a positive author ID or CURSEFORGE_AUTHOR_ID")
		}
	}
	projects := []any{}
	seen := map[int64]bool{}
	total := int64(-1)
	index := int64(0)
	pages := 0
	for index < 10000 {
		query := url.Values{"gameId": {"1"}, "authorId": {strconv.FormatInt(author, 10)}, "index": {strconv.FormatInt(index, 10)}, "pageSize": {"50"}}
		response, err := c.http.JSON(ctx, http.MethodGet, Endpoint(c.core, "/v1/mods/search", query), headers, nil)
		if err != nil {
			return result, err
		}
		rows, ok := response["data"].([]any)
		if !ok || len(rows) > 50 {
			return result, failure("publish.curseforge_data", "invalid projects page")
		}
		pagination, _ := response["pagination"].(map[string]any)
		if raw, ok := pagination["totalCount"]; ok {
			n, ok := positiveOrZero(raw)
			if !ok || n > 10000 || total >= 0 && n != total {
				return result, failure("publish.curseforge_pagination", "project total changed or exceeded the API bound")
			}
			total = n
		}
		for key, want := range map[string]int64{"index": index, "resultCount": int64(len(rows))} {
			if raw, ok := pagination[key]; ok {
				n, valid := positiveOrZero(raw)
				if !valid || n != want {
					return result, failure("publish.curseforge_pagination", "page does not match the request")
				}
			}
		}
		pages++
		for _, row := range rows {
			o, ok := row.(map[string]any)
			if !ok {
				return result, failure("publish.curseforge_data", "invalid project object")
			}
			id, valid := positiveOrZero(o["id"])
			if !valid || id == 0 || seen[id] {
				return result, failure("publish.curseforge_pagination", "missing or repeated project ID")
			}
			seen[id] = true
			project := map[string]any{}
			for _, key := range []string{"id", "name", "slug", "status", "dateCreated", "dateModified"} {
				project[key] = o[key]
			}
			projects = append(projects, project)
		}
		index += int64(len(rows))
		if total >= 0 && (index > total || len(rows) == 0 && index < total) {
			return result, failure("publish.curseforge_pagination", "incomplete project pagination")
		}
		if total >= 0 && index == total || total < 0 && len(rows) == 0 {
			result.Complete = true
			break
		}
	}
	if !result.Complete {
		return result, failure("publish.curseforge_pagination", "project listing reached the API bound")
	}
	result.Data = map[string]any{"author_id": author, "projects": projects, "pages": pages, "total_count": len(projects), "author_scope": "member"}
	return result, nil
}
func positiveOrZero(v any) (int64, bool) {
	n, ok := v.(json.Number)
	if !ok {
		return 0, false
	}
	i, err := n.Int64()
	return i, err == nil && i >= 0
}

type artifact struct {
	file               *os.File
	name, sha256, sha1 string
	size               int64
}

func inspectArtifact(ctx context.Context, filePath string) (artifact, error) {
	var a artifact
	f, err := openArtifact(filePath)
	if err != nil {
		return a, invalid("$.file", "cannot open selected archive")
	}
	success := false
	defer func() {
		if !success {
			f.Close()
		}
	}()
	stat, err := f.Stat()
	if err != nil || !stat.Mode().IsRegular() || stat.Size() == 0 {
		return a, invalid("$.file", "archive must be a nonempty regular file")
	}
	z, err := zip.NewReader(f, stat.Size())
	if err != nil || len(z.File) == 0 {
		return a, invalid("$.file", "expected a nonempty ZIP")
	}
	for _, entry := range z.File {
		n := strings.ReplaceAll(entry.Name, "\\", "/")
		clean := path.Clean(n)
		if strings.HasPrefix(n, "/") || clean == ".." || strings.HasPrefix(clean, "../") || strings.Contains(n, ":") || entry.Mode()&os.ModeSymlink != 0 {
			return a, invalid("$.file", "archive contains an unsafe path")
		}
	}
	h256, h1 := sha256.New(), sha1.New()
	buf := make([]byte, 128<<10)
	for {
		if err = ctx.Err(); err != nil {
			return a, err
		}
		n, e := f.Read(buf)
		if n > 0 {
			h256.Write(buf[:n])
			h1.Write(buf[:n])
		}
		if e == io.EOF {
			break
		}
		if e != nil {
			return a, invalid("$.file", "cannot hash archive")
		}
	}
	if _, err = f.Seek(0, io.SeekStart); err != nil {
		return a, err
	}
	success = true
	return artifact{file: f, name: filepath.Base(filePath), size: stat.Size(), sha256: hex.EncodeToString(h256.Sum(nil)), sha1: hex.EncodeToString(h1.Sum(nil))}, nil
}

type cfReceipt struct {
	Schema         string    `json:"schema"`
	ProjectID      int64     `json:"projectId"`
	Request        string    `json:"request"`
	FileID         int64     `json:"fileId,omitempty"`
	Archive        string    `json:"archive"`
	SHA256         string    `json:"sha256"`
	SHA1           string    `json:"sha1"`
	Bytes          int64     `json:"bytes"`
	MetadataSHA256 string    `json:"metadataSha256"`
	Submission     string    `json:"submission"`
	Verification   string    `json:"verification,omitempty"`
	CreatedAt      time.Time `json:"createdAt"`
}

func validRequest(id string) bool {
	if len(id) == 0 || len(id) > 128 {
		return false
	}
	for _, r := range id {
		if !(r >= 'a' && r <= 'z' || r >= 'A' && r <= 'Z' || r >= '0' && r <= '9' || r == '-' || r == '_') {
			return false
		}
	}
	return true
}
func cfReceiptPath(home string, project int64, request string) (string, error) {
	if project < 1 || !validRequest(request) {
		return "", invalid("$request", "request must contain 1-128 ASCII letters, digits, underscores or hyphens")
	}
	return filepath.Join(home, "publish", "curseforge", "uploads", strconv.FormatInt(project, 10), request+".json"), nil
}
func readCFReceipt(home string, project int64, request string) (cfReceipt, error) {
	var r cfReceipt
	file, err := cfReceiptPath(home, project, request)
	if err != nil {
		return r, err
	}
	raw, err := os.ReadFile(file)
	if err != nil {
		return r, err
	}
	if len(raw) > 65536 {
		return r, failure("publish.receipt_invalid", "upload receipt exceeds its bound")
	}
	d := json.NewDecoder(bytes.NewReader(raw))
	d.DisallowUnknownFields()
	if err = d.Decode(&r); err != nil {
		return r, failure("publish.receipt_invalid", "invalid upload receipt")
	}
	if d.Decode(new(any)) != io.EOF {
		return r, failure("publish.receipt_invalid", "upload receipt contains trailing data")
	}
	if r.Schema != "lycheedev.publish.curseforge-upload.v1" || r.ProjectID != project || r.Request != request {
		return r, failure("publish.receipt_identity", "receipt identity differs from requested upload")
	}
	return r, nil
}
func (r cfReceipt) result(action string) Result {
	data := map[string]any{"project_id": r.ProjectID, "file_id": r.FileID, "archive": r.Archive, "sha256": r.SHA256, "bytes": r.Bytes, "replayAllowed": false}
	if r.Submission != "rejected" {
		data["nextCommand"] = "publish curseforge plugin verify"
	}
	return Result{Platform: "curseforge", Action: action, Complete: r.Verification == "remote_file_identity", OperationID: r.Request, Submission: r.Submission, Verification: r.Verification, Data: data}
}
func saveCFReceipt(ctx context.Context, home string, r cfReceipt) error {
	file, err := cfReceiptPath(home, r.ProjectID, r.Request)
	if err != nil {
		return err
	}
	raw, err := json.Marshal(r)
	if err != nil {
		return err
	}
	return vault.ReplaceFile(ctx, file, raw)
}
func (c *cfClient) uploadFile(ctx context.Context, input map[string]any, requestID string) (Result, error) {
	result := Result{Platform: "curseforge", Action: "plugin upload"}
	project := Integer(input, "project_id")
	if _, err := cfReceiptPath(c.home, project, requestID); err != nil {
		return result, err
	}
	headers, err := c.headers("CURSEFORGE_UPLOAD_TOKEN", "X-Api-Token")
	if err != nil {
		return result, err
	}
	a, err := inspectArtifact(ctx, String(input, "file"))
	if err != nil {
		return result, err
	}
	defer a.file.Close()
	metadata := map[string]any{}
	for field, wire := range map[string]string{"changelog": "changelog", "changelog_type": "changelogType", "display_name": "displayName", "game_versions": "gameVersions", "game_version_names": "gameVersionNames", "release_type": "releaseType", "parent_file_id": "parentFileID", "is_marked_for_manual_release": "isMarkedForManualRelease"} {
		if v, ok := input[field]; ok {
			metadata[wire] = v
		}
	}
	if relations, ok := input["relations"].(map[string]any); ok {
		items := []any{}
		for _, item := range relations["projects"].([]any) {
			o := item.(map[string]any)
			r := map[string]any{"slug": o["slug"], "type": o["type"]}
			if v, ok := o["project_id"]; ok {
				r["projectID"] = v
			}
			items = append(items, r)
		}
		metadata["relations"] = map[string]any{"projects": items}
	}
	raw, _ := json.Marshal(metadata)
	sum := sha256.Sum256(raw)
	lease, err := vault.AcquireLease(ctx, filepath.Join(c.home, "publish", "locks"), "curseforge/"+strconv.FormatInt(project, 10))
	if err != nil {
		return result, err
	}
	defer lease.Close()
	previous, err := readCFReceipt(c.home, project, requestID)
	if err == nil {
		if previous.SHA256 != a.sha256 || previous.MetadataSHA256 != hex.EncodeToString(sum[:]) {
			return previous.result("plugin upload"), invalid("$request", "request already belongs to different original input")
		}
		if previous.Submission == "rejected" {
			return previous.result("plugin upload"), failure("publish.curseforge_rejected", "original upload was rejected; it will not be replayed")
		}
		return previous.result("plugin upload"), nil
	}
	if !errors.Is(err, os.ErrNotExist) {
		return result, err
	}
	receipt := cfReceipt{Schema: "lycheedev.publish.curseforge-upload.v1", ProjectID: project, Request: requestID, Archive: a.name, SHA256: a.sha256, SHA1: a.sha1, Bytes: a.size, MetadataSHA256: hex.EncodeToString(sum[:]), Submission: "intent_recorded", CreatedAt: time.Now().UTC()}
	if err = saveCFReceipt(ctx, c.home, receipt); err != nil {
		return result, err
	}
	response, err := c.http.Multipart(ctx, fmt.Sprintf("%s/api/projects/%d/upload-file", c.upload, project), headers, map[string]string{"metadata": string(raw)}, "file", a.file)
	if err != nil {
		var platformError *Error
		if errors.As(err, &platformError) && platformError.Code == "publish.http_status" {
			details, _ := platformError.Details.(map[string]any)
			status, _ := details["httpStatus"].(int)
			if status == http.StatusUnauthorized || status == http.StatusForbidden {
				receipt.Submission = "rejected"
				if saveErr := saveCFReceipt(context.WithoutCancel(ctx), c.home, receipt); saveErr != nil {
					return receipt.result("plugin upload"), saveErr
				}
			}
		}
		return receipt.result("plugin upload"), err
	}
	id, ok := positiveOrZero(response["id"])
	if !ok || id < 1 {
		return receipt.result("plugin upload"), failure("publish.curseforge_data", "upload response lacks a positive file ID")
	}
	receipt.FileID = id
	receipt.Submission = "acknowledged"
	receipt.Verification = "upload_response"
	if err = saveCFReceipt(context.WithoutCancel(ctx), c.home, receipt); err != nil {
		return receipt.result("plugin upload"), &Error{Code: "publish.receipt_pending", Message: "upload acknowledged; original receipt needs saving", Exit: 6, Details: map[string]any{"project_id": project, "request": requestID, "file_id": id, "replayAllowed": false}}
	}
	return receipt.result("plugin upload"), nil
}
func (c *cfClient) verify(ctx context.Context, project int64, requestID string, fileID int64) (Result, error) {
	result := Result{Platform: "curseforge", Action: "plugin verify"}
	headers, err := c.headers("CURSEFORGE_API_KEY", "x-api-key")
	if err != nil {
		return result, err
	}
	lease, err := vault.AcquireLease(ctx, filepath.Join(c.home, "publish", "locks"), "curseforge/"+strconv.FormatInt(project, 10))
	if err != nil {
		return result, err
	}
	defer lease.Close()
	r, err := readCFReceipt(c.home, project, requestID)
	if err != nil {
		return result, err
	}
	if r.Submission == "rejected" {
		return r.result("plugin verify"), failure("publish.curseforge_rejected", "original upload was rejected; an existing file does not verify that upload")
	}
	if r.FileID != 0 && r.FileID != fileID {
		return r.result("plugin verify"), invalid("$.file_id", "does not match original acknowledgement")
	}
	payload, err := c.http.JSON(ctx, http.MethodGet, fmt.Sprintf("%s/v1/mods/%d/files/%d", c.core, project, fileID), headers, nil)
	if err != nil {
		return r.result("plugin verify"), err
	}
	file, ok := payload["data"].(map[string]any)
	if !ok {
		return r.result("plugin verify"), failure("publish.curseforge_data", "file response has no object")
	}
	id, _ := positiveOrZero(file["id"])
	mod, _ := positiveOrZero(file["modId"])
	length, _ := positiveOrZero(file["fileLength"])
	matches := false
	if hashes, ok := file["hashes"].([]any); ok {
		for _, v := range hashes {
			h, ok := v.(map[string]any)
			if !ok {
				continue
			}
			algo, _ := positiveOrZero(h["algo"])
			value, _ := h["value"].(string)
			matches = matches || algo == 1 && strings.EqualFold(value, r.SHA1)
		}
	}
	if id != fileID || mod != project || length != r.Bytes || !matches {
		return r.result("plugin verify"), failure("publish.upload_mismatch", "remote file differs from original project, bytes or SHA-1")
	}
	r.FileID = fileID
	r.Submission = "acknowledged"
	r.Verification = "remote_file_identity"
	if err = saveCFReceipt(ctx, c.home, r); err != nil {
		return r.result("plugin verify"), err
	}
	return r.result("plugin verify"), nil
}
