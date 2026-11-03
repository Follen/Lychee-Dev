package publish

import (
	"context"
	"strings"
)

func ddLocalPlan(ctx context.Context, action string, in map[string]any) (map[string]any, error) {
	artifacts := []any{}
	form := ddClone(in)
	resource, _, _ := strings.Cut(action, " ")
	for _, key := range []string{"file", "logo_file", "detail_img_files", "display_img_files", "wa_file"} {
		paths := ddSlice(in[key])
		if p := String(in, key); p != "" {
			paths = []any{p}
		}
		for _, v := range paths {
			file := ddText(v)
			if key == "wa_file" {
				raw, err := ddReadFile(file, 32<<20)
				if err != nil {
					return nil, err
				}
				parsed, err := parseDDWA(ctx, string(raw))
				if err != nil {
					return nil, err
				}
				artifacts = append(artifacts, map[string]any{"path": file, "waIdentity": parsed})
				form["content"] = string(raw)
				continue
			}
			if action == "backup upload" {
				b, err := ddLoadBundle(file)
				if err != nil {
					return nil, err
				}
				artifacts = append(artifacts, map[string]any{"path": file, "components": len(b.bundle.Artifacts)})
				b.close()
				continue
			}
			kind := resource + " archive"
			if key != "file" || String(in, "kind") == "image" {
				kind = resource + " image"
			}
			a, err := ddOpenUpload(file, kind)
			if err != nil {
				return nil, err
			}
			a.file.Close()
			artifacts = append(artifacts, map[string]any{"path": file, "sha256": a.digest, "bytes": a.size})
		}
	}
	if resource == "wa" && String(form, "content") != "" {
		if _, err := parseDDWA(ctx, String(form, "content")); err != nil {
			return nil, err
		}
	}
	if strings.HasSuffix(action, " create") && (resource == "plugin" || resource == "config" || resource == "wa") {
		if err := ddCommercial(form, in, resource, true); err != nil {
			return nil, err
		}
		if resource == "wa" && form["category_ids"] == nil {
			form["category_ids"] = []any{"ui_original"}
		}
		if err := ddRequiredForm(resource, form, in); err != nil {
			return nil, err
		}
	}
	if n, present := in["price_fen"]; present {
		v := ddInteger(n)
		if v > 0 && v < 10 {
			return nil, invalid("$.price_fen", "use zero or 10..20000")
		}
	}
	if action == "backup delete-many" && in["confirm_delete"] != true {
		return nil, invalid("$.confirm_delete", "must be true")
	}
	if action == "backup rename" {
		if strings.TrimSpace(String(in, "name")) == "" || len([]rune(String(in, "name"))) > 14 {
			return nil, invalid("$.name", "name must contain 1 to 14 characters")
		}
	}
	return map[string]any{"artifacts": artifacts}, nil
}
