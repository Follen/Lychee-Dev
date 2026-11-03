//go:build windows

package publish

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"unicode/utf8"
	"unsafe"

	"golang.org/x/sys/windows"
)

func modusMainClient(ctx context.Context) (*modusClient, error) {
	root, err := windows.KnownFolderPath(windows.FOLDERID_RoamingAppData, 0)
	if err != nil {
		return nil, capability("publish.modus_session", "cannot locate ModUs main-client storage")
	}
	values, err := readModusLocalStorage(ctx, filepath.Join(root, "modus", "Local Storage", "leveldb"))
	if err != nil {
		return nil, err
	}
	var user struct {
		Token string `json:"token"`
	}
	var app struct {
		Device string `json:"deviceId"`
	}
	if json.Unmarshal([]byte(values["user"]), &user) != nil || strings.TrimSpace(user.Token) == "" || strings.ContainsAny(user.Token, "\r\n\x00") {
		return nil, capability("publish.modus_session", "ModUs main-client login is required")
	}
	if values["app"] != "" && json.Unmarshal([]byte(values["app"]), &app) != nil {
		return nil, capability("publish.modus_session", "invalid ModUs device identity")
	}
	if strings.ContainsAny(app.Device, "\r\n\x00") {
		return nil, capability("publish.modus_session", "invalid ModUs device identity")
	}
	return &modusClient{http: NewHTTP(), origin: "https://app.modus.cool/api/", static: "https://cdn.modus.cool/modus/client_static_api/", token: user.Token, device: app.Device, main: true}, nil
}

func modusCreatorToken() (string, error) {
	root, err := windows.KnownFolderPath(windows.FOLDERID_LocalAppData, 0)
	if err != nil {
		return "", capability("publish.modus_credentials", "cannot locate Creator credentials")
	}
	return readModusCreatorToken(filepath.Join(root, "ModUs.Creator", "auth"))
}
func readModusCreatorToken(directory string) (string, error) {
	filename := filepath.Join(directory, "token.dat")
	f, err := os.Open(filename)
	legacy := false
	if errors.Is(err, os.ErrNotExist) {
		legacy = true
		f, err = os.Open(filepath.Join(directory, "token.json"))
	}
	if err != nil {
		return "", capability("publish.modus_credentials", "Creator login is required")
	}
	defer f.Close()
	raw, err := io.ReadAll(io.LimitReader(f, 65537))
	if err != nil || len(raw) == 0 || len(raw) > 65536 {
		return "", capability("publish.modus_credentials", "invalid Creator credential file")
	}
	if legacy {
		var doc struct {
			Token       string `json:"token"`
			AccessToken string `json:"accessToken"`
		}
		if json.Unmarshal(raw, &doc) != nil {
			return "", capability("publish.modus_credentials", "invalid Creator credential file")
		}
		if doc.Token != "" {
			raw = []byte(doc.Token)
		} else {
			raw = []byte(doc.AccessToken)
		}
	} else {
		entropy := []byte("ModUs.Creator.TokenStore.v1")
		input := windows.DataBlob{Size: uint32(len(raw)), Data: &raw[0]}
		ent := windows.DataBlob{Size: uint32(len(entropy)), Data: &entropy[0]}
		var output windows.DataBlob
		err = windows.CryptUnprotectData(&input, nil, &ent, 0, nil, windows.CRYPTPROTECT_UI_FORBIDDEN, &output)
		runtime.KeepAlive(raw)
		runtime.KeepAlive(entropy)
		if err != nil {
			return "", capability("publish.modus_credentials", "Creator credentials belong to another Windows user or are damaged")
		}
		defer windows.LocalFree(windows.Handle(uintptr(unsafe.Pointer(output.Data))))
		if output.Size == 0 || output.Size > 65536 {
			return "", capability("publish.modus_credentials", "invalid decrypted Creator credential")
		}
		raw = append([]byte(nil), unsafe.Slice(output.Data, int(output.Size))...)
	}
	if !utf8.Valid(raw) {
		return "", capability("publish.modus_credentials", "invalid Creator credential encoding")
	}
	token := strings.TrimSpace(string(raw))
	if token == "" || strings.ContainsAny(token, "\r\n\x00") {
		return "", capability("publish.modus_credentials", "Creator credential is empty or invalid")
	}
	return token, nil
}
