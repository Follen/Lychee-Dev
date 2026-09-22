package delivery

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"os"
	"path/filepath"
	"testing"
)

func payloadFixture(t *testing.T) (string, []Resource) {
	t.Helper()
	root := t.TempDir()
	files := map[string]string{"addon/Core.lua": "local name, ns = ...\n", "skill/SKILL.md": "---\nname: lycheedev\n---\n"}
	var inventory []Resource
	for name, content := range files {
		file := filepath.Join(root, filepath.FromSlash(name))
		if err := os.MkdirAll(filepath.Dir(file), 0700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(file, []byte(content), 0600); err != nil {
			t.Fatal(err)
		}
		digest := sha256.Sum256([]byte(content))
		inventory = append(inventory, Resource{name, int64(len(content)), hex.EncodeToString(digest[:])})
	}
	return root, inventory
}

func TestPayloadCompleteInventory(t *testing.T) {
	root, inventory := payloadFixture(t)
	if err := VerifyPayload(context.Background(), root, inventory); err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if err := VerifyPayload(ctx, root, inventory); !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
}

func TestPayloadRejectsChanges(t *testing.T) {
	for _, kind := range []string{"missing", "extra", "corrupt", "size", "duplicate", "collision", "digest", "escape", "budget"} {
		t.Run(kind, func(t *testing.T) {
			root, inventory := payloadFixture(t)
			file := filepath.Join(root, filepath.FromSlash(inventory[0].Path))
			var err error
			switch kind {
			case "missing":
				err = os.Remove(file)
			case "extra":
				err = os.WriteFile(filepath.Join(root, "skill", "local-secret.txt"), []byte("secret"), 0600)
			case "corrupt":
				err = os.WriteFile(file, make([]byte, inventory[0].Bytes), 0600)
			case "size":
				inventory[0].Bytes++
			case "duplicate":
				inventory = append(inventory, inventory[0])
			case "collision":
				extra := inventory[0]
				extra.Path += "/child"
				inventory = append(inventory, extra)
			case "digest":
				inventory[0].SHA256 = "abcd"
			case "escape":
				inventory[0].Path = "addon/../../outside"
			case "budget":
				inventory[0].Bytes = maxPayloadBytes + 1
			}
			if err != nil {
				t.Fatal(err)
			}
			if err = VerifyPayload(context.Background(), root, inventory); !errors.Is(err, ErrPayload) {
				t.Fatalf("got %v", err)
			}
		})
	}
}

func TestPortableResourcePaths(t *testing.T) {
	for _, name := range []string{"addon/core.lua", "skill/references/数据.md"} {
		if !resourcePath(name) {
			t.Errorf("rejected %q", name)
		}
	}
	for _, name := range []string{"../addon/x", "addon/../x", "addon//x", "addon/x.", "addon/x ", "addon/CON.lua", "addon/COM1", "addon/LPT².txt", "addon/a:b", "addon/a\\b", "skill/x\x00", "native/a", "/addon/x", "addon/.lycheedev-install.json", "skill/.LYCHEEDEV-INSTALL.JSON"} {
		if resourcePath(name) {
			t.Errorf("accepted %q", name)
		}
	}
}

func TestPayloadRejectsLinks(t *testing.T) {
	root, inventory := payloadFixture(t)
	out := filepath.Join(t.TempDir(), "outside")
	if err := os.WriteFile(out, []byte("outside"), 0600); err != nil {
		t.Fatal(err)
	}
	link := filepath.Join(root, "skill", "linked")
	if err := os.Symlink(out, link); err != nil {
		t.Skipf("file symlinks unavailable: %v", err)
	}
	if err := VerifyPayload(context.Background(), root, inventory); !errors.Is(err, ErrPayload) {
		t.Fatal(err)
	}
}
