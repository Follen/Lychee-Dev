// Historical records only: no game capture, input, or execution adapters.
package live

import (
	"github.com/follenfang/lycheedev/internal/desktop"
	"os"
	"path/filepath"
	"testing"
)

func makeTestClient(t *testing.T, gameRoot, folder, flavor, version string) string {
	t.Helper()
	client := filepath.Join(gameRoot, folder)
	if err := os.MkdirAll(client, 0700); err != nil {
		t.Fatal(err)
	}
	writeTestFile(t, filepath.Join(client, ".flavor.info"), "Product Flavor!STRING:0\n"+flavor+"\n")
	writeTestFile(t, filepath.Join(client, "version.txt"), version+"\n")
	writeTestFile(t, filepath.Join(client, "Wow.exe"), "fixture executable\n")
	return client
}

func writeTestFile(t *testing.T, path, content string) {
	t.Helper()
	if err := os.WriteFile(path, []byte(content), 0600); err != nil {
		t.Fatal(err)
	}
}

func testWindow(handle uint64, client string) desktop.WindowIdentity {
	return desktop.WindowIdentity{Handle: handle, ProcessID: uint32(1000 + handle), ProcessStartedAt: 55, Executable: filepath.Join(client, "Wow.exe"), Class: "OsWindow", Title: "World of Warcraft"}
}
