package bridge

import (
	"bytes"
	"compress/flate"
	"context"
	"encoding/json"
	"github.com/follenfang/lycheedev/internal/desktop"
	"io"
	"strings"
	"testing"
)

func TestOpticalPairPreservesIndependentProofs(t *testing.T) {
	receipt := Signal{Schema: "lycheedev.signal.v1", Kind: "reported", Release: "2.0.2", Product: "classic", Build: "5.5.4.69934", RequestID: "R-test", Sequence: 8, ReportBytes: 13, ReportAdler32: "209c046d"}
	nonce := strings.Repeat("a", 32)
	for _, mode := range []string{"valid", "wrong-schema", "old-sequence", "zero-epoch", "oversized-epoch", "fraction", "foreign-nonce", "missing-nonce", "extra-tuple", "unknown-field", "duplicate-field", "wrong-kind"} {
		t.Run(mode, func(t *testing.T) {
			r := receipt
			ready := []any{nonce, uint64(9), uint64(2)}
			wire := map[string]any{"schema": "lycheedev.receipt.v1", "receipt": r, "ready": ready}
			switch mode {
			case "wrong-schema":
				wire["schema"] = "other"
			case "old-sequence":
				ready[1] = 8
			case "zero-epoch":
				ready[2] = 0
			case "oversized-epoch":
				ready[2] = uint64(9007199254740992)
			case "fraction":
				ready[1] = 9.5
			case "foreign-nonce":
				r.SessionNonce = strings.Repeat("b", 32)
			case "missing-nonce":
				ready[0] = ""
			case "extra-tuple":
				wire["ready"] = append(ready, true)
			case "unknown-field":
				wire["inputReady"] = true
			case "wrong-kind":
				r.Kind = "ready"
				r.SessionNonce = nonce
				r.RequestID = ""
				r.ReportBytes = 0
				r.ReportAdler32 = ""
			}
			wire["receipt"] = r
			raw, err := json.Marshal(wire)
			if err != nil {
				t.Fatal(err)
			}
			if mode == "duplicate-field" {
				raw = append([]byte(`{"schema":"other",`), raw[1:]...)
			}
			signals, err := ParseOpticalSignals(raw)
			if mode != "valid" {
				if err == nil {
					t.Fatalf("accepted %s: %+v", mode, signals)
				}
				return
			}
			if err != nil || len(signals) != 2 {
				t.Fatalf("pair: %+v %v", signals, err)
			}
			r.SessionNonce = nonce
			if signals[0] != r || signals[1].Kind != "ready" || signals[1].RequestID != "" || signals[1].ReportBytes != 0 || !signals[1].InputReady || signals[1].Sequence != 9 || signals[1].RuntimeEpoch != 2 {
				t.Fatalf("changed proof: %+v", signals)
			}
			if signals[0].Match(SignalExpectation{SessionNonce: strings.Repeat("b", 32)}) == nil {
				t.Fatal("paired nonce overwritten by caller")
			}
		})
	}
}

func TestOpticalPairReaderRequiresFreshFrameForEachProof(t *testing.T) {
	raw := []byte(`{"schema":"lycheedev.receipt.v1","receipt":{"schema":"lycheedev.signal.v1","kind":"reported","release":"2.0.2","product":"classic","build":"5.5.4.69934","requestId":"R-test","sequence":8,"reportBytes":13,"reportAdler32":"209c046d"},"ready":["aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",9,2]}`)
	feed := &queuedFrames{frames: []*desktop.CapturedFrame{opticalFrame(t, raw, 1), opticalFrame(t, raw, 1), opticalFrame(t, raw, 2)}, end: io.EOF}
	reader := ObserveSignals(feed)
	reader.SetIdentityBaseline(SignalIdentity{Character: "次年雪", Realm: "祈福", GUID: "Player-1-2", SessionNonce: strings.Repeat("a", 32)})
	e := SignalExpectation{Release: "2.0.2", Kind: "reported", SessionNonce: strings.Repeat("a", 32), RequestID: "R-test", Product: "classic", Build: "5.5.4.69934", Character: "次年雪", Realm: "祈福"}
	report, err := reader.WaitForSignal(context.Background(), e)
	if err != nil {
		t.Fatal(err)
	}
	e.Kind, e.RequestID, e.AfterSequence, e.RequireInputReady = "ready", "", report.Sequence, true
	ready, err := reader.WaitForSignal(context.Background(), e)
	if err != nil || ready.RuntimeEpoch != 2 || ready.GUID != "Player-1-2" || len(feed.frames) != 0 {
		t.Fatalf("fresh ready: %+v %v remaining=%d", ready, err, len(feed.frames))
	}
}

func TestSignalInflationStopsAtBudget(t *testing.T) {
	var compressed bytes.Buffer
	w, _ := flate.NewWriter(&compressed, flate.BestCompression)
	_, _ = w.Write(bytes.Repeat([]byte("x"), 1<<20))
	if err := w.Close(); err != nil {
		t.Fatal(err)
	}
	decoded, err := inflateSignal(compressed.Bytes())
	if err != nil || len(decoded) != 4097 {
		t.Fatalf("expanded past budget: %d %v", len(decoded), err)
	}
	if _, err = ParseSignal(append([]byte{signalDeflateMarker}, compressed.Bytes()...)); err == nil {
		t.Fatal("oversized signal accepted")
	}
}
