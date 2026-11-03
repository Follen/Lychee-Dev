package publish

import (
	"context"
	"crypto/md5"
	"encoding/hex"
	"encoding/json"
	"io"
	"net/http"
	"net/url"
	"strconv"
	"strings"
)

func (c *modusClient) continueOperation(ctx context.Context, home string, op *modusOperation) error {
	if op.Verified {
		return nil
	}
	if op.Submission == "rejected" {
		return failure("publish.modus_rejected", "the original write was rejected and will not be replayed")
	}
	if op.Submission == "uncertain" {
		return &Error{Code: "publish.modus_pending", Message: "original stage requires readback; automatic replay is disabled", Exit: 6}
	}
	if modusMainAction(op.Action) {
		return c.continueMain(ctx, home, op)
	}
	if op.Action == "plugin upload" || op.Action == "plugin edit" {
		return c.continueRelease(ctx, home, op)
	}
	if op.Stage == "prepare" {
		var err error
		if op.Action == "project create" || op.Action == "project edit" {
			op.Wire, err = c.prepareProject(ctx, op.Input, op.Action == "project create")
			if err != nil {
				return err
			}
		}
		if op.Action == "project edit" || op.Action == "project delete" {
			op.RemoteID = fmtModusID(op.Input["project_id"])
		}
		if op.Action == "plugin delete" {
			op.RemoteID = fmtModusID(op.Input["file_id"])
		}
		if strings.HasSuffix(op.Action, "delete") && Integer(op.Input, "project_id") < 200000000 {
			return invalid("$.project_id", "restricted Creator projects do not permit deletion or release changes")
		}
		op.Stage = "write"
		if err = saveModusOperation(ctx, home, *op); err != nil {
			return err
		}
	}
	if op.Stage == "write" {
		route := ""
		switch op.Action {
		case "project create":
			route = "game/data/author/project/release"
		case "project edit":
			route = "game/data/author/project/update"
		case "project delete":
			route = "game/data/author/project/delete/project/" + op.RemoteID
		case "plugin delete":
			route = "game/data/author/project/delete"
			op.Wire = map[string]any{"projectId": op.Input["project_id"], "fileId": op.Input["file_id"]}
		default:
			return invalid("$action", "unsupported original write action")
		}
		op.Submission = "uncertain"
		if err := saveModusOperation(ctx, home, *op); err != nil {
			return err
		}
		v, err := c.call(ctx, "POST", route, op.Wire, nil)
		if err != nil {
			return modusWriteFailure(ctx, home, op, err)
		}
		if op.Action == "project create" {
			op.RemoteID = modusResponseID(v, "projectId", "id")
		}
		op.Submission = "submitted"
		op.Stage = "readback"
		if err = saveModusOperation(ctx, home, *op); err != nil {
			return err
		}
	}
	if op.Stage != "readback" {
		return failure("publish.modus_operation", "unknown saved operation stage")
	}
	err := c.verifyOperation(ctx, op)
	if e := saveModusOperation(ctx, home, *op); e != nil {
		return e
	}
	return err
}
func modusResponseID(v any, keys ...string) string {
	if obj, ok := v.(map[string]any); ok {
		for _, key := range keys {
			if id := fmtModusID(obj[key]); id != "" {
				return id
			}
		}
		return ""
	}
	return fmtModusID(v)
}
func (c *modusClient) continueRelease(ctx context.Context, home string, op *modusOperation) error {
	project := Integer(op.Input, "project_id")
	if project < 200000000 {
		return invalid("$.project_id", "restricted Creator projects do not permit release changes")
	}
	projectID := strconv.FormatInt(project, 10)
	var a artifact
	if file := String(op.Input, "file"); file != "" {
		var err error
		a, err = inspectArtifact(ctx, file)
		if err != nil {
			return err
		}
		defer a.file.Close()
		if op.ArchiveSHA256 != "" && op.ArchiveSHA256 != a.sha256 {
			return invalid("$.file", "original archive changed; reuse requires original bytes")
		}
	}
	if op.Stage == "prepare" {
		wire := map[string]any{"projectId": op.Input["project_id"]}
		for _, k := range []string{"version", "type", "changelog"} {
			if v, ok := op.Input[k]; ok {
				if k == "changelog" && op.Action == "plugin edit" {
					v = modusNull(v)
				}
				wire[k] = v
			}
		}
		if a.file != nil {
			m, err := inspectModusZIP(ctx, a)
			if err != nil {
				return err
			}
			config, err := c.read(ctx, "options game-versions", map[string]any{"keys": []any{"wow_builds"}})
			if err != nil {
				return err
			}
			var supplied []modusGameVersion
			if values, ok := op.Input["supported_game_versions"].([]any); ok {
				for _, v := range values {
					obj := v.(map[string]any)
					supplied = append(supplied, modusGameVersion{String(obj, "gameVersion"), String(obj, "server")})
				}
			}
			games, err := selectModusGames(m.Games, config, supplied)
			if err != nil {
				return err
			}
			h := md5.New()
			if _, err = io.Copy(h, a.file); err != nil {
				return invalid("$.file", "cannot hash archive")
			}
			if _, err = a.file.Seek(0, io.SeekStart); err != nil {
				return err
			}
			op.ArchiveSHA256 = a.sha256
			op.ArchiveMD5 = hex.EncodeToString(h.Sum(nil))
			op.ArchiveBytes = a.size
			wire["md5"] = op.ArchiveMD5
			wire["zipSize"] = a.size
			wire["unzipSize"] = m.UnzipSize
			wire["tocVersion"] = m.TOCVersion
			wire["supportedGameVersionsReqs"] = games
		} else if values, ok := op.Input["supported_game_versions"].([]any); ok {
			config, err := c.read(ctx, "options game-versions", map[string]any{"keys": []any{"wow_builds"}})
			if err != nil {
				return err
			}
			selected := make([]modusGameVersion, 0, len(values))
			for _, v := range values {
				obj := v.(map[string]any)
				selected = append(selected, modusGameVersion{String(obj, "gameVersion"), String(obj, "server")})
			}
			games, err := selectModusGames(selected, config, selected)
			if err != nil {
				return err
			}
			wire["supportedGameVersionsReqs"] = games
		}
		op.Wire = wire
		if op.Action == "plugin edit" {
			op.RemoteID = fmtModusID(op.Input["file_id"])
			if _, err := c.read(ctx, "plugin get", op.Input); err != nil {
				return err
			}
			wire["fileId"] = op.Input["file_id"]
			op.Stage = "metadata"
		} else {
			op.Stage = "reserve"
		}
		if err := saveModusOperation(ctx, home, *op); err != nil {
			return err
		}
	}
	if op.Stage == "reserve" {
		op.Submission = "uncertain"
		if err := saveModusOperation(ctx, home, *op); err != nil {
			return err
		}
		v, err := c.call(ctx, "GET", "game/data/author/project/fileId/"+projectID, nil, nil)
		if err != nil {
			return modusWriteFailure(ctx, home, op, err)
		}
		op.RemoteID = modusResponseID(v, "fileId", "id")
		if op.RemoteID == "" {
			return failure("publish.modus_file_id", "platform did not return the reserved file ID")
		}
		op.Stage = "metadata"
		op.Submission = "not_submitted"
		if err = saveModusOperation(ctx, home, *op); err != nil {
			return err
		}
	}
	if op.Stage == "metadata" {
		if a.file != nil {
			op.Wire["path"] = "modus/wow/addons/" + projectID + "/files/" + op.RemoteID + ".zip"
		}
		op.Submission = "uncertain"
		if err := saveModusOperation(ctx, home, *op); err != nil {
			return err
		}
		route := "game/data/author/project/upload"
		if op.Action == "plugin edit" {
			route = "game/data/author/project/file/update"
		}
		if _, err := c.call(ctx, "POST", route, op.Wire, nil); err != nil {
			return modusWriteFailure(ctx, home, op, err)
		}
		op.Stage = "readback"
		if a.file != nil {
			op.Stage = "binary"
		}
		op.Submission = "submitted"
		if err := saveModusOperation(ctx, home, *op); err != nil {
			return err
		}
	}
	if op.Stage == "binary" {
		if a.file == nil {
			return invalid("$.file", "original archive is required for upload continuation")
		}
		v, err := c.call(ctx, "GET", "game/data/author/project/file/upload/signature/"+projectID+"/"+op.RemoteID, nil, nil)
		if err != nil {
			return err
		}
		signed := fmtString(v)
		if obj, ok := v.(map[string]any); ok {
			signed = String(obj, "signedUrl")
		}
		u, err := url.Parse(signed)
		if err != nil || u.Scheme != "https" || u.Hostname() == "" || u.User != nil || u.Fragment != "" {
			return failure("publish.modus_upload_url", "invalid signed upload endpoint")
		}
		op.Submission = "uncertain"
		if err = saveModusOperation(ctx, home, *op); err != nil {
			return err
		}
		req, err := http.NewRequestWithContext(ctx, "PUT", signed, a.file)
		if err != nil {
			return failure("publish.modus_upload_url", "invalid signed upload endpoint")
		}
		req.ContentLength = a.size
		req.Header.Set("Content-Type", "application/zip")
		response, err := c.http.client.Do(req)
		if err != nil {
			if ctx.Err() != nil {
				return ctx.Err()
			}
			return failure("publish.modus_upload", "archive upload requires verification")
		}
		response.Body.Close()
		if response.StatusCode < 200 || response.StatusCode >= 300 {
			return failure("publish.modus_upload", "archive upload returned unsuccessful status")
		}
		op.Stage = "readback"
		op.Submission = "submitted"
		if err = saveModusOperation(ctx, home, *op); err != nil {
			return err
		}
	}
	if op.Stage != "readback" {
		return failure("publish.modus_operation", "unknown saved release stage")
	}
	err := c.verifyOperation(ctx, op)
	if e := saveModusOperation(ctx, home, *op); e != nil {
		return e
	}
	return err
}
func (c *modusClient) verifyOperation(ctx context.Context, op *modusOperation) error {
	if op.Submission == "rejected" {
		return failure("publish.modus_rejected", "the original write was rejected and will not be replayed")
	}
	if op.Submission == "not_submitted" || op.Stage == "prepare" || op.Stage == "reserve" {
		return &Error{Code: "publish.modus_pending", Message: "original content has not been submitted; resume its saved operation before readback", Exit: 6}
	}
	if modusMainAction(op.Action) {
		return c.verifyMain(ctx, op)
	}
	if op.RemoteID == "" {
		return &Error{Code: "publish.modus_pending", Message: "remote identity is missing; provide the original remote ID for readback", Exit: 6}
	}
	if strings.HasSuffix(op.Action, "delete") {
		// A missing detail or HTTP error is insufficient evidence of deletion.
		resource := "project list"
		if op.Action == "plugin delete" {
			resource = "plugin list"
		}
		seen := map[string]bool{}
		var expectedTotal int64 = -1
		for page := int64(1); page <= 1000; page++ {
			in := map[string]any{"page_num": json.Number(strconv.FormatInt(page, 10)), "page_size": json.Number("100"), "project_id": op.Input["project_id"]}
			v, err := c.read(ctx, resource, in)
			if err != nil {
				return err
			}
			rows, err := modusRows(v)
			if err != nil {
				return err
			}
			obj, ok := v.(map[string]any)
			if !ok {
				return failure("publish.modus_verify", "deletion requires complete list pagination")
			}
			total, ok := positiveOrZero(obj["total"])
			if !ok {
				return failure("publish.modus_verify", "deletion requires a total count")
			}
			if expectedTotal >= 0 && total != expectedTotal {
				return failure("publish.modus_verify", "deletion list changed during pagination")
			}
			expectedTotal = total
			for _, row := range rows {
				id := modusResponseID(row, "projectId", "id")
				if op.Action == "plugin delete" {
					id = modusResponseID(row, "fileId", "id")
				}
				if id == "" || seen[id] {
					return failure("publish.modus_verify", "deletion list has missing or duplicate identities")
				}
				seen[id] = true
				if id == op.RemoteID {
					return &Error{Code: "publish.modus_pending", Message: "remote resource is still present", Exit: 6}
				}
			}
			if int64(len(seen)) > total {
				return failure("publish.modus_verify", "deletion list exceeds its total count")
			}
			if int64(len(seen)) == total {
				op.Verified = true
				op.Stage = "complete"
				return nil
			}
			if len(rows) != 100 {
				return failure("publish.modus_verify", "incomplete deletion pagination")
			}
		}
		return failure("publish.modus_verify", "deletion pagination exceeded bound")
	}
	input := map[string]any{"project_id": op.Input["project_id"]}
	readAction := "project get"
	if op.Action == "project create" {
		input["project_id"] = json.Number(op.RemoteID)
	}
	if strings.HasPrefix(op.Action, "plugin ") {
		readAction = "plugin get"
		input["file_id"] = json.Number(op.RemoteID)
	}
	v, err := c.read(ctx, readAction, input)
	if err != nil {
		return err
	}
	obj, ok := v.(map[string]any)
	if !ok {
		return failure("publish.modus_verify", "remote detail is invalid")
	}
	for k, want := range op.Wire {
		if k == "projectId" || k == "fileId" {
			continue
		}
		if k == "screenshotBase64sReqs" {
			if data, ok := want.(map[string]any); ok && String(data, "screenshotBase64s") == "" {
				continue
			}
			return &Error{Code: "publish.modus_pending", Message: "project logo requires media readback", Exit: 6}
		}
		if k == "imagesOps" {
			return &Error{Code: "publish.modus_pending", Message: "project image changes require media readback", Exit: 6}
		}
		actual := obj[k]
		if k == "supportedGameVersionsReqs" {
			if actual == nil {
				actual = obj["supportedGameVersions"]
			}
		}
		if !modusSameField(k, want, actual) {
			return &Error{Code: "publish.modus_pending", Path: k, Message: "saved field differs from original request", Exit: 6}
		}
	}
	if op.ArchiveSHA256 != "" && op.Stage == "metadata" {
		op.Stage = "binary"
		op.Submission = "submitted"
		return &Error{Code: "publish.modus_pending", Message: "original metadata verified; resume to upload the retained original archive", Exit: 6}
	}
	if op.ArchiveSHA256 != "" {
		rawPath := String(obj, "path")
		endpoint, err := modusDownloadURL(rawPath)
		if err != nil {
			return err
		}
		req, err := http.NewRequestWithContext(ctx, "GET", endpoint, nil)
		if err != nil {
			return err
		}
		response, err := c.http.client.Do(req)
		if err != nil {
			return failure("publish.modus_verify", "cannot read public package")
		}
		defer response.Body.Close()
		if response.StatusCode != 200 {
			return failure("publish.modus_verify", "public package returned unsuccessful status")
		}
		h := md5.New()
		n, err := io.Copy(h, io.LimitReader(response.Body, op.ArchiveBytes+1))
		if err != nil || n != op.ArchiveBytes || hex.EncodeToString(h.Sum(nil)) != op.ArchiveMD5 {
			return &Error{Code: "publish.modus_pending", Message: "public package bytes differ from original archive", Exit: 6}
		}
	}
	op.Verified = true
	op.Stage = "complete"
	op.Submission = "submitted"
	return nil
}
func modusDownloadURL(p string) (string, error) {
	if p == "" || strings.TrimSpace(p) != p || strings.ContainsAny(p, "\r\n\x00") {
		return "", failure("publish.modus_verify", "invalid public package path")
	}
	if !strings.Contains(p, "://") {
		p = "https://cdn.modus.cool/" + strings.TrimLeft(p, "/")
	}
	u, err := url.Parse(p)
	if err != nil || u.Scheme != "https" || u.Hostname() != "cdn.modus.cool" || (u.Port() != "" && u.Port() != "443") || u.User != nil || u.RawQuery != "" || u.Fragment != "" {
		return "", failure("publish.modus_verify", "package origin requires explicit separate verification")
	}
	return u.String(), nil
}
