package publish

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"errors"
	"hash/crc64"
	"io"
	"net/http"
	"strconv"
	"strings"
	"testing"
)

func bbBackupReply(v any) *http.Response {
	return bbTestResponse(200, bbJSON(map[string]any{"status": "ok", "result": v}))
}
func bbBackupOperationFixture(t *testing.T) (*bbClient, *bbOperation) {
	t.Helper()
	in, _ := bbBackupFixture(t)
	r, e := bbLocalBackup(context.Background(), "backup generate", in)
	if e != nil {
		t.Fatal(e)
	}
	c := newBB(t.TempDir())
	c.session = &bbSession{Method: "sms", Identity: "123", Device: "fixture"}
	input := map[string]any{"file": String(r.(map[string]any), "file")}
	op := &bbOperation{Schema: "lycheedev.blackbox.operation.v1", ID: "backup-test", Action: "backup upload", Account: "123", Method: "sms", Input: input, Digest: bbDigest(input)}
	if e = c.prepareBackup(context.Background(), op); e != nil {
		t.Fatal(e)
	}
	return c, op
}
func TestBlackboxBackupCOSAndSaveRecoveryNoReplay(t *testing.T) {
	ctx := context.Background()
	c, op := bbBackupOperationFixture(t)
	allocations, puts, heads, saves := 0, 0, 0, 0
	objects := map[string]map[string]string{}
	var remote map[string]any
	c.http.Transport = bbTestTransport(func(r *http.Request) (*http.Response, error) {
		if strings.Contains(r.URL.Host, ".myqcloud.com") {
			if r.Header.Get("Cookie") != "" {
				t.Fatal("workshop cookie leaked to COS")
			}
			if r.Method == "PUT" {
				puts++
				raw, e := io.ReadAll(r.Body)
				if e != nil {
					t.Fatal(e)
				}
				hash := r.Header.Get("Content-MD5")
				b, _ := base64.StdEncoding.DecodeString(hash)
				md5 := fmtHex(b)
				objects[r.URL.Path] = map[string]string{"md5": md5, "size": strconv.Itoa(len(raw))}
				if puts == 1 {
					return nil, errors.New("lost upload response")
				}
				res := bbTestResponse(200, "")
				res.Header.Set("ETag", md5)
				res.Header.Set("x-cos-hash-crc64ecma", strconv.FormatUint(crc64.Checksum(raw, crc64.MakeTable(crc64.ECMA)), 10))
				return res, nil
			}
			if r.Method == "HEAD" {
				heads++
				v := objects[r.URL.Path]
				res := bbTestResponse(200, "")
				res.Header.Set("ETag", v["md5"])
				res.Header.Set("Content-Length", v["size"])
				return res, nil
			}
			t.Fatal(r.Method)
		}
		switch r.URL.Path {
		case "/wow/create_backup/":
			allocations++
			return bbBackupReply(map[string]any{"ID": "backup-id", "MaxSize": 1000000}), nil
		case "/wow/get_user_backup_credential/":
			return bbBackupReply(map[string]any{"cos": "https://fixture-123.cos.ap-shanghai.myqcloud.com", "host": "https://wow.max-c.com", "path": "backups/" + r.URL.Query().Get("file_type") + ".zip", "Credentials": map[string]any{"Credentials": map[string]any{"TmpSecretID": "fixture-id", "TmpSecretKey": "fixture-key", "Token": "fixture-token"}}}), nil
		case "/wow/save_user_configuration/":
			saves++
			r.ParseForm()
			raw, e := base64.StdEncoding.DecodeString(r.PostForm.Get("configuration"))
			if e != nil {
				t.Fatal(e)
			}
			d := json.NewDecoder(strings.NewReader(string(raw)))
			d.UseNumber()
			if e = d.Decode(&remote); e != nil {
				t.Fatal(e)
			}
			return nil, errors.New("lost save response")
		case "/wow/get_user_configuration_backup_list/":
			return bbBackupReply(map[string]any{"backupList": []any{remote}}), nil
		}
		t.Fatalf("unexpected route %s", r.URL)
		return nil, nil
	})
	if e := c.continueBackup(ctx, op); e == nil {
		t.Fatal("lost PUT response hidden")
	}
	if puts != 1 || allocations != 1 {
		t.Fatal(puts, allocations)
	}
	op, e := c.loadOperation(op.ID)
	if e != nil {
		t.Fatal(e)
	}
	if e = c.continueBackup(ctx, op); e == nil {
		t.Fatal("lost save response hidden")
	}
	if puts != 4 || heads != 1 || saves != 1 || allocations != 1 {
		t.Fatal(puts, heads, saves, allocations, e)
	}
	op, e = c.loadOperation(op.ID)
	if e != nil {
		t.Fatal(e)
	}
	if e = c.continueBackup(ctx, op); e != nil {
		t.Fatal(e)
	}
	if !op.Verified || saves != 1 || puts != 4 || allocations != 1 {
		t.Fatal("mutation replayed or unverified", saves, puts, allocations)
	}
}
func fmtHex(b []byte) string {
	const digits = "0123456789abcdef"
	out := make([]byte, len(b)*2)
	for i, v := range b {
		out[2*i] = digits[v>>4]
		out[2*i+1] = digits[v&15]
	}
	return string(out)
}
func TestBlackboxBackupAllocationUncertaintyAndLimitPersist(t *testing.T) {
	for _, mode := range []string{"lost", "limit"} {
		t.Run(mode, func(t *testing.T) {
			c, op := bbBackupOperationFixture(t)
			calls := 0
			c.http.Transport = bbTestTransport(func(r *http.Request) (*http.Response, error) {
				calls++
				if r.URL.Path != "/wow/create_backup/" {
					t.Fatal("upload proceeded after invalid allocation")
				}
				if mode == "lost" {
					return nil, errors.New("lost allocation")
				}
				return bbBackupReply(map[string]any{"ID": "allocated", "MaxSize": 1}), nil
			})
			if e := c.continueBackup(context.Background(), op); e == nil {
				t.Fatal("allocation failure hidden")
			}
			op, e := c.loadOperation(op.ID)
			if e != nil {
				t.Fatal(e)
			}
			if e = c.continueBackup(context.Background(), op); e == nil {
				t.Fatal("allocation replayed or limit bypassed")
			}
			if calls != 1 {
				t.Fatal(calls)
			}
		})
	}
}
func TestBlackboxBackupDeleteBlocksReferencedAndResumesRemaining(t *testing.T) {
	ctx := context.Background()
	c := newBB(t.TempDir())
	c.session = &bbSession{Method: "sms", Identity: "123", Device: "fixture"}
	deleted := map[string]bool{}
	writes := map[string]int{}
	referenced := true
	c.http.Transport = bbTestTransport(func(r *http.Request) (*http.Response, error) {
		switch r.URL.Path {
		case "/wow/get_user_configuration_backup_list/":
			rows := []any{}
			for _, id := range []string{"a", "b"} {
				if !deleted[id] {
					rows = append(rows, map[string]any{"ID": id, "Name": id, "IsClassic": false})
				}
			}
			return bbBackupReply(map[string]any{"backupList": rows}), nil
		case "/wow/v2/share/list/":
			rows := []any{}
			if referenced && r.URL.Query().Get("offset") == "0" {
				rows = append(rows, map[string]any{"ShareID": "share", "BackupID": "a"})
			}
			return bbBackupReply(map[string]any{"shares": rows}), nil
		case "/wow/remove_user_configuration_backup/":
			id := r.URL.Query().Get("backup_id")
			writes[id]++
			deleted[id] = true
			if id == "a" {
				return nil, errors.New("lost delete response")
			}
			return bbBackupReply(map[string]any{}), nil
		}
		t.Fatal(r.URL.Path)
		return nil, nil
	})
	input := map[string]any{"backup_ids": []any{"a", "b"}, "confirm_delete": true}
	op := &bbOperation{Schema: "lycheedev.blackbox.operation.v1", ID: "delete-test", Action: "backup delete", Account: "123", Method: "sms", Input: input, Digest: bbDigest(input)}
	if e := c.prepareBackup(ctx, op); e == nil {
		t.Fatal("referenced backup allowed")
	}
	if len(writes) != 0 {
		t.Fatal(writes)
	}
	referenced = false
	if e := c.prepareBackup(ctx, op); e != nil {
		t.Fatal(e)
	}
	if e := c.continueBackup(ctx, op); e == nil {
		t.Fatal("lost delete hidden")
	}
	op, e := c.loadOperation(op.ID)
	if e != nil {
		t.Fatal(e)
	}
	if e = c.continueBackup(ctx, op); e != nil {
		t.Fatal(e)
	}
	if !op.Verified || writes["a"] != 1 || writes["b"] != 1 {
		t.Fatal(writes, op.Verified)
	}
}

func TestBlackboxModuleCOSResumeOnlyHeadsOriginalObject(t *testing.T) {
	ctx := context.Background()
	c, backup := bbBackupOperationFixture(t)
	source := backup.Backup.Package.Archives[0]
	archive, e := bbSnapshot(ctx, c.home, "module-test", source.Local)
	if e != nil {
		t.Fatal(e)
	}
	input := map[string]any{"file": source.Local}
	op := &bbOperation{Schema: "lycheedev.blackbox.operation.v1", ID: "module-test", Action: "plugin update", Account: "123", Method: "sms", Input: input, Digest: bbDigest(input), Archive: archive}
	tokens, puts, heads := 0, 0, 0
	c.http.Transport = bbTestTransport(func(r *http.Request) (*http.Response, error) {
		if r.URL.Path == "/wow/cos/upload/token/" {
			tokens++
			return bbBackupReply(map[string]any{"info": map[string]any{"region": "ap-shanghai", "bucket": "fixture-123", "host": "wow.max-c.com", "Credentials": map[string]any{"TmpSecretID": "id", "TmpSecretKey": "key", "Token": "token"}}, "files": []any{map[string]any{"key": "wow/module/original.zip"}}}), nil
		}
		if r.URL.Host != "fixture-123.cos.ap-shanghai.myqcloud.com" || r.URL.Path != "/wow/module/original.zip" {
			t.Fatal("object identity changed", r.URL)
		}
		switch r.Method {
		case "PUT":
			puts++
			io.Copy(io.Discard, r.Body)
			return nil, errors.New("lost response")
		case "HEAD":
			heads++
			res := bbTestResponse(200, "")
			res.Header.Set("ETag", source.MD5)
			res.Header.Set("Content-Length", strconv.FormatInt(source.Bytes, 10))
			return res, nil
		}
		t.Fatal(r.Method)
		return nil, nil
	})
	if _, e = c.uploadModule(ctx, op); e == nil {
		t.Fatal("uncertainty hidden")
	}
	op, e = c.loadOperation(op.ID)
	if e != nil {
		t.Fatal(e)
	}
	u, e := c.uploadModule(ctx, op)
	if e != nil {
		t.Fatal(e)
	}
	if u != "https://wow.max-c.com/wow/module/original.zip" || tokens != 1 || puts != 1 || heads != 1 {
		t.Fatal(u, tokens, puts, heads)
	}
}

func TestBlackboxBackupCreatePreservesInputAndSnapshots(t *testing.T) {
	in, _ := bbBackupFixture(t)
	c := newBB(t.TempDir())
	op := &bbOperation{ID: "create-test", Action: "backup create", Input: bbClone(in), Digest: bbDigest(in)}
	before := op.Digest
	if e := c.prepareBackup(context.Background(), op); e != nil {
		t.Fatal(e)
	}
	if bbDigest(op.Input) != before || op.Backup.Package == nil || len(op.Backup.Package.Archives) != 4 {
		t.Fatal("input changed or package missing")
	}
	for _, a := range op.Backup.Package.Archives {
		if !strings.HasPrefix(a.Local, c.home) {
			t.Fatal("archive not pinned", a.Local)
		}
	}
}
