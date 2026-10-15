package protocol_test

import (
	"os/exec"
	"path/filepath"
	"runtime"
	"testing"
)

func TestLuaCaptureWriter(t *testing.T) {
	lua := luaRuntime(t)

	_, testFile, _, ok := runtime.Caller(0)
	if !ok {
		t.Fatal("runtime.Caller failed")
	}
	testDir := filepath.Dir(testFile)
	writerScript := filepath.Join(testDir, "writer.lua")
	captureWriter := filepath.Join(testDir, "..", "..", "addon", "Bridge", "CaptureWriter.lua")
	output, err := exec.Command(lua, writerScript, captureWriter).CombinedOutput()
	if err != nil {
		t.Fatalf("%v\n%s", err, output)
	}
	if string(output) != "capture-writer: passed\n" && string(output) != "capture-writer: passed\r\n" {
		t.Fatalf("unexpected output: %s", output)
	}
}
