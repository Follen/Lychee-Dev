package protocol_test

import (
	"encoding/json"
	"os/exec"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/bridge"
)

func TestAtomicLiveCommandsAcrossClientCatalogs(t *testing.T) {
	output, err := exec.Command(luaRuntime(t), "atomic_live.lua", "../../addon", wowGlobals(t)).CombinedOutput()
	if err != nil {
		t.Fatalf("%v\n%s", err, output)
	}
	var receipts []string
	if err := json.Unmarshal(output, &receipts); err != nil {
		t.Fatal(err)
	}
	if len(receipts) != 12 {
		t.Fatalf("missing atomic live receipts: %d", len(receipts))
	}
	for index, text := range receipts {
		signal, err := bridge.ParseSignal([]byte(text))
		if err != nil {
			t.Fatal(err)
		}
		switch index % 3 {
		case 0:
			if signal.Kind != "ready" || signal.RequestID != "RELOAD-"+strings.Repeat("b", 32) || signal.RuntimeEpoch != 2 || signal.ReportBytes != 0 {
				t.Fatalf("invalid standalone reload receipt: %+v", signal)
			}
		case 1:
			if signal.Kind != "ready" || signal.RequestID != "BUGS-A" || signal.RuntimeEpoch != 3 || signal.ReportBytes != 0 {
				t.Fatalf("invalid bugs reentry receipt: %+v", signal)
			}
		case 2:
			if signal.Kind != "acknowledged" || signal.RequestID != "BUGS-A" || signal.RuntimeEpoch != 0 || signal.ReportBytes == 0 {
				t.Fatalf("invalid bugs acknowledgement: %+v", signal)
			}
		}
	}
}
