package publish

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"io/fs"
	"net/url"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strings"
	"time"

	"github.com/follenfang/lycheedev/internal/vault"
)

type newBeeCLI struct {
	executable string
	prefix     []string
}

func discoverNewBeeCLI(explicit string) (newBeeCLI, error) {
	candidates := []string{}
	if explicit != "" {
		candidates = append(candidates, explicit)
	} else {
		if p, err := exec.LookPath("ncc.exe"); err == nil {
			candidates = append(candidates, p)
		}
		if appdata := os.Getenv("APPDATA"); appdata != "" {
			candidates = append(candidates, filepath.Join(appdata, "npm", "node_modules", "@newbeebox", "newbeebox-creator-center-cli", "dist", "ncc.cjs"))
		}
		if p, err := exec.LookPath("ncc"); err == nil {
			candidates = append(candidates, p)
		}
	}
	for _, path := range candidates {
		absolute, err := filepath.Abs(path)
		if err != nil {
			continue
		}
		info, err := os.Stat(absolute)
		if err != nil || !info.Mode().IsRegular() {
			continue
		}
		ext := strings.ToLower(filepath.Ext(absolute))
		if ext == ".cjs" || ext == ".js" {
			node, err := exec.LookPath("node")
			if err != nil {
				return newBeeCLI{}, capability("publish.newbee_node", "official ncc requires Node.js")
			}
			return newBeeCLI{node, []string{absolute}}, nil
		}
		if ext == ".cmd" || ext == ".bat" || ext == ".ps1" {
			continue
		}
		return newBeeCLI{executable: absolute}, nil
	}
	return newBeeCLI{}, capability("publish.newbee_cli", "install the official @newbeebox/newbeebox-creator-center-cli or supply cli_path pointing to ncc.cjs")
}

type newBeeOutput struct {
	bytes.Buffer
	exceeded bool
}

func (b *newBeeOutput) Write(p []byte) (int, error) {
	n := len(p)
	remaining := maxResponseBytes - b.Len()
	if remaining < len(p) {
		b.exceeded = true
		p = p[:max(remaining, 0)]
	}
	_, _ = b.Buffer.Write(p)
	return n, nil
}

func (cli newBeeCLI) run(ctx context.Context, args []string) (any, error) {
	ctx, cancel := context.WithTimeout(ctx, 10*time.Minute)
	defer cancel()
	argv := append(append([]string{}, cli.prefix...), args...)
	cmd := exec.CommandContext(ctx, cli.executable, argv...)
	cmd.Stdin = strings.NewReader("")
	var out newBeeOutput
	cmd.Stdout = &out
	cmd.Stderr = io.Discard
	err := cmd.Run()
	if ctx.Err() != nil {
		return nil, ctx.Err()
	}
	if err != nil {
		return nil, failure("publish.newbee_cli_failed", "official CLI failed; inspect the original resource before repeating a mutation")
	}
	if out.exceeded {
		return nil, failure("publish.newbee_output", "official CLI output exceeds 8 MiB")
	}
	var value any
	d := json.NewDecoder(bytes.NewReader(out.Bytes()))
	d.UseNumber()
	if err = d.Decode(&value); err != nil {
		return nil, failure("publish.newbee_output", "official CLI returned invalid JSON")
	}
	if err = d.Decode(new(any)); err != io.EOF {
		return nil, failure("publish.newbee_output", "official CLI returned trailing output")
	}
	if m, ok := value.(map[string]any); ok {
		if _, exists := m["error"]; exists {
			return nil, failure("publish.newbee_cli_failed", "official CLI reported an error")
		}
	}
	return newBeePublic(value), nil
}

// CLI responses can contain desktop tokens, raw WA/config bodies or signed URLs.
// These are not part of the publishing result or durable operation receipt.
func newBeePublic(value any) any {
	switch v := value.(type) {
	case map[string]any:
		out := map[string]any{}
		for key, item := range v {
			lower := strings.ToLower(strings.ReplaceAll(key, "_", ""))
			if lower == "args" || lower == "argv" || lower == "arguments" || lower == "tcontent" || strings.Contains(lower, "wastr") || strings.Contains(lower, "wacode") {
				continue
			}
			if strings.Contains(lower, "token") || strings.Contains(lower, "password") || strings.Contains(lower, "secret") || strings.Contains(lower, "authorization") || strings.Contains(lower, "cookie") || strings.Contains(lower, "proof") || lower == "wastr" || lower == "wastring" || lower == "wacode" || lower == "content" || lower == "body" || lower == "code" {
				continue
			}
			out[key] = newBeePublic(item)
		}
		return out
	case []any:
		out := make([]any, len(v))
		for i, item := range v {
			out[i] = newBeePublic(item)
		}
		return out
	case string:
		if u, err := url.Parse(v); err == nil && (u.Scheme == "http" || u.Scheme == "https") {
			u.RawQuery = ""
			u.Fragment = ""
			u.User = nil
			return u.String()
		}
		return v
	default:
		return value
	}
}

func newBeeArgs(c newBeeOfficialCommand, input map[string]any) ([]string, error) {
	if _, ok := input["yes"]; ok && !Bool(input, "yes") {
		return nil, invalid("$.yes", "sync requires true to replace the selected local project")
	}
	if len(c.args) == 3 && c.args[1] == "uipack" && c.args[2] == "push" {
		if presentCount(input, []string{"file", "title", "intro"}) == 0 {
			return nil, invalid("$", "provide file, title or intro")
		}
	}
	args := append([]string{}, c.args...)
	for _, name := range c.positional {
		if v, ok := input[name]; ok {
			if strings.HasPrefix(fmt.Sprint(v), "-") {
				return nil, invalid("$."+name, "positional arguments must not begin with '-'")
			}
			args = append(args, fmt.Sprint(v))
		}
	}
	keys := make([]string, 0, len(c.flags))
	for key := range c.flags {
		keys = append(keys, key)
	}
	sort.Strings(keys)
	for _, key := range keys {
		if v, ok := input[key]; ok {
			if b, ok := v.(bool); ok {
				if b {
					args = append(args, c.flags[key])
				}
			} else {
				args = append(args, c.flags[key], fmt.Sprint(v))
			}
		}
	}
	return append(args, "-o", "json"), nil
}

func newBeeFingerprint(ctx context.Context, action string, input map[string]any) (string, error) {
	h := sha256.New()
	raw, _ := json.Marshal(map[string]any{"action": action, "input": input})
	h.Write(raw)
	hashFile := func(path string) error {
		if err := ctx.Err(); err != nil {
			return err
		}
		file, err := openArtifact(path)
		if err != nil {
			return invalid("$input", "cannot read selected content file")
		}
		defer file.Close()
		info, err := file.Stat()
		if err != nil || !info.Mode().IsRegular() {
			return invalid("$input", "content must be a regular file")
		}
		if action == "creator plugin upload" {
			ext := strings.ToLower(filepath.Ext(path))
			if info.Size() > 300*1024*1024 || (ext != ".zip" && ext != ".rar" && ext != ".7z") {
				return invalid("$.file", "plugin package must be .zip/.rar/.7z and no larger than 300 MiB")
			}
		}
		fmt.Fprintf(h, "\x00%s\x00%d\x00", path, info.Size())
		_, err = io.Copy(h, file)
		return err
	}
	keys := make([]string, 0, len(input))
	for k := range input {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	for _, key := range keys {
		value, ok := input[key].(string)
		if !ok || value == "" {
			continue
		}
		if key == "directory" && action == "official plugin upload" {
			err := filepath.WalkDir(value, func(path string, entry fs.DirEntry, err error) error {
				if err != nil {
					return invalid("$.directory", "cannot read selected project")
				}
				if entry.Type()&os.ModeSymlink != 0 {
					return invalid("$.directory", "project contains a link")
				}
				if entry.IsDir() {
					return nil
				}
				return hashFile(path)
			})
			if err != nil {
				return "", err
			}
		} else if key == "file" || strings.HasPrefix(value, "@") {
			path := value
			if strings.HasPrefix(value, "@") {
				path = value[1:]
			}
			if err := hashFile(path); err != nil {
				return "", err
			}
		} else if key == "screenshots" || key == "images" {
			for _, item := range strings.Split(value, ",") {
				item = strings.TrimSpace(item)
				if item != "" && !strings.HasPrefix(item, "https://") && !strings.HasPrefix(item, "http://") {
					if err := hashFile(item); err != nil {
						return "", err
					}
				}
			}
		} else if key == "logo" || key == "thumbnail" {
			if !strings.HasPrefix(value, "https://") && !strings.HasPrefix(value, "http://") {
				if err := hashFile(value); err != nil {
					return "", err
				}
			}
		}
	}
	return hex.EncodeToString(h.Sum(nil)), nil
}

type newBeeReceipt struct {
	Session     string            `json:"session,omitempty"`
	Fingerprint string            `json:"fingerprint"`
	State       string            `json:"state"`
	Result      Result            `json:"result"`
	Target      map[string]any    `json:"target,omitempty"`
	Expected    map[string]string `json:"expected,omitempty"`
}

func newBeeTarget(input map[string]any) map[string]any {
	target := map[string]any{}
	for _, key := range []string{"id", "directory", "file", "version", "resource", "backup", "wa_id", "cloud_id", "game_version_list", "game_versions"} {
		if value, ok := input[key]; ok {
			target[key] = value
		}
	}
	return target
}

func runNewBeeOfficial(ctx context.Context, action string, c newBeeOfficialCommand, input map[string]any, request Request) (Result, error) {
	result := Result{Platform: "newbee", Action: action}
	args, err := newBeeArgs(c, input)
	if err != nil {
		return result, err
	}
	cli, err := discoverNewBeeCLI(String(input, "cli_path"))
	if err != nil {
		return result, err
	}
	if action == "official session doctor" {
		result.Complete = true
		result.Data = map[string]any{"executable": cli.executable, "entrypoint": cli.prefix, "channel": "official", "authenticationChecked": false}
		return result, nil
	}
	if !c.write {
		value, err := cli.run(ctx, args)
		result.Data = value
		result.Complete = err == nil
		return result, err
	}
	if request.RequestID == "" {
		return result, invalid("$request", "mutating NewBee commands require --request")
	}
	fingerprint, err := newBeeFingerprint(ctx, action, input)
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
	if data, readErr := os.ReadFile(receiptPath); readErr == nil {
		var receipt newBeeReceipt
		if newBeeDecodeStored(data, &receipt) != nil {
			return result, failure("publish.newbee_receipt", "invalid original operation receipt")
		}
		if receipt.Fingerprint != fingerprint {
			return result, invalid("$request", "request ID is already bound to different input")
		}
		if receipt.State == "complete" {
			return receipt.Result, nil
		}
		result.Submission = "uncertain"
		result.Data = map[string]any{"replayed": false, "next": "read original resource with get/versions and reconcile the original request"}
		return result, &Error{Code: "publish.newbee_pending", Message: "original operation may have submitted; inspect the original resource", Exit: 6}
	} else if !os.IsNotExist(readErr) {
		return result, readErr
	}
	save := func(state string) error {
		data, err := json.Marshal(newBeeReceipt{Fingerprint: fingerprint, State: state, Result: result, Target: newBeeTarget(input)})
		if err != nil {
			return err
		}
		if err = os.MkdirAll(filepath.Dir(receiptPath), 0700); err != nil {
			return err
		}
		return vault.ReplaceFile(ctx, receiptPath, data)
	}
	result.Submission = "uncertain"
	if err = save("submitted"); err != nil {
		return result, err
	}
	value, err := cli.run(ctx, args)
	result.Data = value
	if err != nil {
		return result, err
	}
	result.Complete = true
	result.Submission = "accepted"
	result.Verification = "official-cli-receipt"
	if err = save("complete"); err != nil {
		result.Complete = false
		return result, err
	}
	return result, nil
}
