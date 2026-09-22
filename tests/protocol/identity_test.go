package protocol_test

import (
	"encoding/json"
	"image"
	"image/color"
	"os"
	"os/exec"
	"path/filepath"
	"slices"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/desktop"
)

type identityDrawing struct {
	Width, Height int
	Runs          []string
}

type identityReport struct {
	Product, Build                        string
	First, Refreshed, NoActor, Restricted string
	FirstDrawing, RefreshedDrawing        identityDrawing
}

const identityProbeNonce = "0123456789abcdef0123456789abcdef"

// The four clients trigger the real Identity.lua, display through the real
// ReceiptView, and the host decodes the drawn symbol back into a receipt. The
// Lua-emitted key sets must equal the cross-language samples in protocol/.
func TestIdentityMarkerFourClients(t *testing.T) {
	output, err := exec.Command(luaRuntime(t), "identity.lua", "../../addon").CombinedOutput()
	if err != nil {
		t.Fatalf("%v\n%s", err, output)
	}
	lines := strings.Split(strings.TrimSpace(string(output)), "\n")
	if len(lines) != 4 {
		t.Fatalf("expected four identity reports: %s", output)
	}
	for i, product := range []string{"retail", "classic", "titan", "forever"} {
		t.Run(product, func(t *testing.T) {
			var report identityReport
			if err := json.Unmarshal([]byte(lines[i]), &report); err != nil {
				t.Fatal(err)
			}
			if report.Product != product || report.Build == "" {
				t.Fatalf("installation identity: %+v", report)
			}
			first := parseIdentityReceipt(t, report.First, report.Product, report.Build)
			refreshed := parseIdentityReceipt(t, report.Refreshed, report.Product, report.Build)
			noActor := parseIdentityReceipt(t, report.NoActor, report.Product, report.Build)
			restricted := parseIdentityReceipt(t, report.Restricted, report.Product, report.Build)
			for _, signal := range []bridge.Signal{first, refreshed, noActor, restricted} {
				if signal.Kind != "identity" || signal.ProbeNonce != identityProbeNonce || signal.Sequence != 0 ||
					signal.SessionNonce != "" || signal.RequestID != "" || signal.Release != "2.0.0-dev" {
					t.Fatalf("identity marker: %+v", signal)
				}
			}
			if first.ActorState != "ok" || first.Character != "Paladin" || first.Realm != "Realm" || first.GUID != "Player-1-123" || first.InputReady || first.InputReason != "input_keyboard_focus" {
				t.Fatalf("first display: %+v", first)
			}
			if refreshed.ActorState != "ok" || !refreshed.InputReady || refreshed.InputReason != "" || refreshed.Character != first.Character || refreshed.GUID != first.GUID {
				t.Fatalf("refreshed display: %+v", refreshed)
			}
			if noActor.ActorState != "no_actor" || noActor.Character != "" || noActor.Realm != "" || noActor.GUID != "" {
				t.Fatalf("no_actor marker: %+v", noActor)
			}
			if restricted.ActorState != "actor_restricted" || restricted.Character != "" || restricted.Realm != "" || restricted.GUID != "" {
				t.Fatalf("restricted marker: %+v", restricted)
			}
			for _, shape := range []struct{ receipt, sample string }{
				{report.First, "identity.unready.json"},
				{report.Refreshed, "identity.ok.json"},
				{report.NoActor, "identity.no_actor.json"},
				{report.Restricted, "identity.actor_restricted.json"},
			} {
				raw, err := os.ReadFile(filepath.Join("..", "..", "protocol", "samples", shape.sample))
				if err != nil {
					t.Fatal(err)
				}
				var want map[string]any
				if err := json.Unmarshal(raw, &want); err != nil {
					t.Fatal(err)
				}
				var got map[string]any
				if err := json.Unmarshal([]byte(shape.receipt), &got); err != nil {
					t.Fatal(err)
				}
				if !slices.Equal(sortedKeys(got), sortedKeys(want)) {
					t.Fatalf("Lua-emitted keys %v, sample %s keys %v", sortedKeys(got), shape.sample, sortedKeys(want))
				}
			}
			assertDrawingDecodes(t, report.FirstDrawing, report.First)
			assertDrawingDecodes(t, report.RefreshedDrawing, report.Refreshed)
		})
	}
}

func parseIdentityReceipt(t *testing.T, raw, product, build string) bridge.Signal {
	t.Helper()
	signal, err := bridge.ParseSignal([]byte(raw))
	if err != nil || signal.Product != product || signal.Build != build {
		t.Fatalf("receipt %q: %+v %v", raw, signal, err)
	}
	return signal
}

func sortedKeys(value map[string]any) []string {
	keys := make([]string, 0, len(value))
	for key := range value {
		keys = append(keys, key)
	}
	slices.Sort(keys)
	return keys
}

func assertDrawingDecodes(t *testing.T, drawing identityDrawing, want string) {
	t.Helper()
	runs := parseSymbolRuns(t, drawing.Runs)
	module := 0
	if len(runs) > 0 {
		module = runs[0].h
	}
	if module < 2 || drawing.Width < 29*module || drawing.Width > 1480 || drawing.Height != drawing.Width || len(drawing.Runs) > 16384 {
		t.Fatalf("invalid drawing: %dx%d module %d", drawing.Width, drawing.Height, module)
	}
	pixels := image.NewNRGBA(image.Rect(0, 0, drawing.Width, drawing.Height))
	for y := 0; y < drawing.Height; y++ {
		for x := 0; x < drawing.Width; x++ {
			pixels.SetNRGBA(x, y, color.NRGBA{255, 255, 255, 255})
		}
	}
	for _, run := range runs {
		if symbolRunInvalid(run, module, drawing.Width, drawing.Height) {
			t.Fatalf("quiet zone or run invalid: %+v", run)
		}
		for yy := run.y; yy < run.y+run.h; yy++ {
			for xx := run.x; xx < run.x+run.w; xx++ {
				pixels.SetNRGBA(xx, yy, color.NRGBA{0, 0, 0, 255})
			}
		}
	}
	texts, err := desktop.DecodeSymbols(pixels)
	if err != nil || !slices.Equal(texts, []string{want}) {
		t.Fatalf("host decode mismatch: %v, got %q", err, texts)
	}
}
