package protocol_test

import (
	"bufio"
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"math"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
	"strings"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/live/channel"
)

// Use the production Lua writer, slot protocol and Go driver through durable
// report verification, release and journal recovery, without a game process.
func TestSlotDriverLuaReportPreservesFiniteNumbers(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()
	dir := t.TempDir()
	root, err := filepath.Abs(filepath.Join("..", "..", "addon"))
	if err != nil {
		t.Fatal(err)
	}
	command := exec.CommandContext(ctx, luaRuntime(t), "slot_server.lua", filepath.ToSlash(root), filepath.ToSlash(dir))
	in, err := command.StdinPipe()
	if err != nil {
		t.Fatal(err)
	}
	stdout, err := command.StdoutPipe()
	if err != nil {
		t.Fatal(err)
	}
	var stderr strings.Builder
	command.Stderr = &stderr
	if err := command.Start(); err != nil {
		t.Fatal(err)
	}
	defer func() {
		fmt.Fprintln(in, "quit")
		in.Close()
		if err := command.Wait(); err != nil {
			t.Errorf("peer: %v %s", err, stderr.String())
		}
	}()
	peer := &slotPeer{t: t, dir: dir, in: in, out: bufio.NewScanner(stdout)}
	if !peer.out.Scan() || peer.out.Text() != "ready" {
		t.Fatal("peer not ready")
	}
	identity := channel.Identity{Schema: channel.IdentitySchema, Runtime: strings.Repeat("1", 32), NextSlot: 1, Slots: 500, GUID: "Player-1-123", Character: "Tester", Realm: "Realm", Build: "70000", Product: "retail", Release: "2.5.1"}
	d, err := channel.New(filepath.Join(dir, "connections", "numeric.jsonl"), peer, identity)
	if err != nil {
		t.Fatal(err)
	}
	peer.driver = d
	if err := d.Save(ctx, "created"); err != nil {
		t.Fatal(err)
	}
	if err := d.Connect(ctx); err != nil {
		t.Fatal(err)
	}

	want := []float64{0, math.Copysign(0, -1)}
	expressions := []string{"0", "-1 / math.huge"}
	for _, value := range []float64{
		0.1, 0.5, 1, 1.2345678901234567, 100000000000000.125,
		9007199254740991, 9007199254740992, 9007199254740994,
		math.Nextafter(1, 0), math.Nextafter(1, 2), 1e-200,
		math.Ldexp(1, -1022), math.SmallestNonzeroFloat64, math.MaxFloat64,
	} {
		for _, number := range []float64{value, -value} {
			want = append(want, number)
			expressions = append(expressions, strconv.FormatFloat(number, 'g', -1, 64))
		}
	}
	code := "return {" + strings.Join(expressions, ",") + "}"
	if err := d.PrepareRequest(ctx, "numeric-roundtrip", code, 10, "opaque"); err != nil {
		t.Fatal(err)
	}
	if err := d.Run(ctx); err != nil {
		t.Fatal(err)
	}
	if d.State.Operation.Stage != "complete" || d.State.Operation.CleanupMethod != "released" {
		t.Fatalf("report did not complete and release: %+v", d.State.Operation)
	}
	checkReport := func(data []byte) {
		t.Helper()
		var report struct {
			OK                bool      `json:"ok"`
			ResourcesReleased bool      `json:"resourcesReleased"`
			Result            []float64 `json:"result"`
		}
		if err := json.Unmarshal(data, &report); err != nil {
			t.Fatal(err)
		}
		if !report.OK || !report.ResourcesReleased || len(report.Result) != len(want) {
			t.Fatalf("unexpected report: %s", data)
		}
		for i, number := range report.Result {
			if math.Float64bits(number) != math.Float64bits(want[i]) {
				t.Errorf("result[%d] for Lua %s: got %.17g (%016x), want %.17g (%016x)",
					i, expressions[i], number, math.Float64bits(number), want[i], math.Float64bits(want[i]))
			}
		}
	}
	checkReport(d.State.Operation.Result)
	loaded, err := channel.Load(d.Log, peer)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(loaded.State.Operation.Result, d.State.Operation.Result) {
		t.Fatal("recovered report bytes changed")
	}
	checkReport(loaded.State.Operation.Result)
	peer.driver = loaded
	sends := peer.sends
	if err := loaded.Run(ctx); err != nil || peer.sends != sends {
		t.Fatalf("completed report retry sent input: %v", err)
	}
	if err := loaded.Disconnect(ctx); err != nil {
		t.Fatal(err)
	}
	if loaded.State.Bound || loaded.State.Transaction != nil {
		t.Fatal("connection did not unbind")
	}
	count, err := os.ReadFile(filepath.Join(dir, "executions.txt"))
	if err != nil || string(count) != "1" {
		t.Fatalf("executions=%s: %v", count, err)
	}
}
