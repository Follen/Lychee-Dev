package protocol_test

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"testing"

	"github.com/follenfang/lycheedev/internal/bridge"
)

func TestMemoryRecordLuaParity(t *testing.T) {
	lua := luaRuntime(t)
	root, err := filepath.Abs(filepath.Join("..", "..", "addon"))
	if err != nil {
		t.Fatal(err)
	}
	root = filepath.ToSlash(root)
	output := filepath.Join(t.TempDir(), "record.bin")
	script := fmt.Sprintf(`local ns={};assert(loadfile(%q.."/Bridge/CaptureWriter.lua"))("Lychee Dev",ns);assert(loadfile(%q.."/Bridge/MemoryProtocol.lua"))("Lychee Dev",ns);local file=assert(io.open(%q,"wb"));file:write(assert(ns.MemoryProtocol.Encode(string.rep("1",32),string.rep("2",32),string.rep("3",32),3,3,17,"hello")));file:close()`, root, root, filepath.ToSlash(output))
	log, err := exec.Command(lua, "-e", script).CombinedOutput()
	if err != nil {
		t.Fatalf("%v: %s", err, log)
	}
	b, err := os.ReadFile(output)
	if err != nil {
		t.Fatal(err)
	}
	h, payload, err := bridge.DecodeMemoryRecord(b)
	if err != nil || string(payload) != "hello" || h.Sequence != 17 || h.Nonce[0] != 0x11 || h.Runtime[0] != 0x22 || h.Ticket[0] != 0x33 {
		t.Fatalf("header=%+v payload=%q err=%v", h, payload, err)
	}
}
