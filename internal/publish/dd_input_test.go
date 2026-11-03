package publish

import (
	"encoding/json"
	"strings"
	"testing"
)

func TestDDBackupRenameNameAdmission(t *testing.T) {
	spec, _ := DDInput("backup rename")
	for _, name := range []string{strings.Repeat("名", 14), strings.Repeat("名", 15), " "} {
		raw, err := json.Marshal(map[string]any{"session": "selected", "sn": "backup-1", "name": name})
		if err != nil {
			t.Fatal(err)
		}
		_, err = spec.Decode(raw)
		valid := name == strings.Repeat("名", 14)
		if (err == nil) != valid {
			t.Fatalf("name length=%d valid=%v err=%v", len([]rune(name)), valid, err)
		}
	}
}
