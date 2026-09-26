package bridge

import (
	"context"
	"encoding/json"
	"errors"
	"github.com/follenfang/lycheedev/internal/buildinfo"
	"image"
	"image/color"
	"image/draw"
	"strings"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/makiuchi-d/gozxing"
	"github.com/makiuchi-d/gozxing/qrcode"
)

type queuedFrames struct {
	frames []*desktop.CapturedFrame
	end    error
}

func (f *queuedFrames) Next(ctx context.Context) (*desktop.CapturedFrame, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if len(f.frames) == 0 {
		return nil, f.end
	}
	frame := f.frames[0]
	f.frames = f.frames[1:]
	return frame, nil
}
func signalFrame(t *testing.T, s Signal, ticks int64) *desktop.CapturedFrame {
	t.Helper()
	data, err := json.Marshal(s)
	if err != nil {
		t.Fatal(err)
	}
	return opticalFrame(t, data, ticks)
}

func opticalFrame(t *testing.T, data []byte, ticks int64) *desktop.CapturedFrame {
	t.Helper()
	bitmap, err := qrcode.NewQRCodeWriter().Encode(string(data), gozxing.BarcodeFormat_QR_CODE, 600, 600, map[gozxing.EncodeHintType]interface{}{gozxing.EncodeHintType_CHARACTER_SET: "UTF-8"})
	if err != nil {
		t.Fatal(err)
	}
	pixels := image.NewNRGBA(image.Rect(0, 0, bitmap.GetWidth(), bitmap.GetHeight()))
	for y := 0; y < bitmap.GetHeight(); y++ {
		for x := 0; x < bitmap.GetWidth(); x++ {
			shade := uint8(255)
			if bitmap.Get(x, y) {
				shade = 0
			}
			pixels.SetNRGBA(x, y, color.NRGBA{shade, shade, shade, 255})
		}
	}
	return &desktop.CapturedFrame{NRGBA: pixels, SystemTicks: ticks, ObservedAt: time.Now()}
}
func TestSignalReaderIgnoresForeignAndStaleFrames(t *testing.T) {
	s := Signal{Schema: "lycheedev.signal.v1", Release: "2.0.0", Kind: "ready", SessionNonce: "one", Character: "role", Realm: "realm", Product: "retail", Build: "12.1.0.69875", Sequence: 1, InputReady: true}
	foreign := s
	foreign.SessionNonce = "other"
	first := signalFrame(t, foreign, 1)
	stale := signalFrame(t, s, 1)
	fresh := signalFrame(t, s, 2)
	feed := &queuedFrames{frames: []*desktop.CapturedFrame{first, stale, fresh}, end: errors.New("ended")}
	reader := ObserveSignals(feed)
	got, err := reader.WaitForSignal(context.Background(), SignalExpectation{Release: s.Release, Kind: s.Kind, SessionNonce: s.SessionNonce, Character: s.Character, Realm: s.Realm, Product: s.Product, Build: s.Build, RequireInputReady: true})
	if err != nil || got.SessionNonce != "one" || len(feed.frames) != 0 {
		t.Fatalf("%+v %v remaining=%d", got, err, len(feed.frames))
	}
	if err := reader.RequireFreshSignal(); err != nil {
		t.Fatal(err)
	}
	if _, err := reader.WaitForSignal(context.Background(), SignalExpectation{Release: s.Release, Kind: s.Kind, SessionNonce: s.SessionNonce, Character: s.Character, Realm: s.Realm, Product: s.Product, Build: s.Build}); err == nil {
		t.Fatal("empty feed observed a signal")
	}
	if err := reader.RequireFreshSignal(); err == nil {
		t.Fatal("failed observation retained prior permission")
	}
}

func TestSignalFreshnessIncludesPostObservationDelay(t *testing.T) {
	for _, at := range []time.Time{time.Time{}, time.Now().Add(-4 * time.Second), time.Now().Add(time.Second)} {
		reader := &SignalReader{observedAt: at}
		if err := reader.RequireFreshSignal(); err == nil {
			t.Fatal("invalid frame age accepted")
		}
	}
	var absent *SignalReader
	if err := absent.RequireFreshSignal(); err == nil {
		t.Fatal("nil reader claimed freshness")
	}
}
func TestMissingFramesNeverProveAcknowledgement(t *testing.T) {
	missing := errors.New("no frame")
	reader := ObserveSignals(&queuedFrames{end: missing})
	_, err := reader.WaitForSignal(context.Background(), SignalExpectation{Release: "2.0.0", Kind: "acknowledged", SessionNonce: "one", Character: "role", Realm: "realm", Product: "retail", Build: "12.1.0.69875"})
	if !errors.Is(err, missing) {
		t.Fatalf("missing frame became a receipt: %v", err)
	}
}

func TestSignalReaderSelectsReportThenReadinessFromPairedDisplay(t *testing.T) {
	report := Signal{Schema: "lycheedev.signal.v1", Release: buildinfo.Version, Kind: "reported", SessionNonce: "one", RequestID: "REQ-one", Character: "role", Realm: "realm", Product: "retail", Build: "12.1.0.69875", Sequence: 3, ReportBytes: 2, ReportAdler32: "00000001"}
	ready := report
	ready.Kind, ready.RequestID, ready.GUID = "ready", "", "Player-1-123"
	ready.Sequence, ready.InputReady, ready.ReportBytes, ready.ReportAdler32 = 4, true, 0, ""
	left, right := signalFrame(t, report, 1), signalFrame(t, ready, 1)
	pixels := image.NewNRGBA(image.Rect(0, 0, 1200, 600))
	draw.Draw(pixels, image.Rect(0, 0, 600, 600), left.NRGBA, image.Point{}, draw.Src)
	draw.Draw(pixels, image.Rect(600, 0, 1200, 600), right.NRGBA, image.Point{}, draw.Src)
	feed := &queuedFrames{end: errors.New("ended")}
	reader := ObserveSignals(feed)
	for i, want := range []Signal{report, ready} {
		// A new capture is required for each observation, even when the screen
		// has not changed. The adjacent symbol must not make it ambiguous.
		feed.frames = []*desktop.CapturedFrame{{NRGBA: pixels, SystemTicks: int64(i + 1), ObservedAt: time.Now()}}
		expected := SignalExpectation{Release: want.Release, Kind: want.Kind, SessionNonce: want.SessionNonce, RequestID: want.RequestID, Character: want.Character, Realm: want.Realm, Product: want.Product, Build: want.Build, AfterSequence: want.Sequence - 1, RequireInputReady: want.InputReady}
		got, err := reader.WaitForSignal(context.Background(), expected)
		if err != nil || got != want {
			t.Fatalf("paired %s: got %+v, error %v", want.Kind, got, err)
		}
		if err := reader.RequireFreshSignal(); err != nil {
			t.Fatal(err)
		}
	}
}

func TestReadyDiscoveryRequiresSelectedBuildAndUnambiguousFreshPixels(t *testing.T) {
	signal := Signal{Schema: "lycheedev.signal.v1", Release: buildinfo.Version, Kind: "ready", SessionNonce: strings.Repeat("a", 32), Character: "Paladin", Realm: "Realm", GUID: "Player-1-123", Product: "retail", Build: "12.1.0.69875", Sequence: 1, InputReady: true}
	want := SignalExpectation{Kind: "ready", Release: signal.Release, Product: signal.Product, Build: signal.Build, RequireInputReady: true}
	end := errors.New("no more frames")
	for _, mode := range []string{"valid", "filtered", "wrong-build", "wrong-release", "wrong-actor", "not-ready", "old-frame", "ambiguous", "incomplete", "not-discovery"} {
		t.Run(mode, func(t *testing.T) {
			candidate, expected := signal, want
			switch mode {
			case "filtered":
				expected.Character, expected.Realm = signal.Character, signal.Realm
			case "wrong-build":
				candidate.Build = "12.1.0.69876"
			case "wrong-release":
				candidate.Release = "1.2.0"
			case "wrong-actor":
				expected.Character = "Other"
			case "not-ready":
				candidate.InputReady = false
			case "incomplete":
				expected.Build = ""
			case "not-discovery":
				expected.Kind = "reported"
			}
			frame := signalFrame(t, candidate, 1)
			if mode == "old-frame" {
				frame.ObservedAt = time.Now().Add(-2 * time.Second)
			}
			if mode == "ambiguous" {
				other := signal
				other.SessionNonce = strings.Repeat("b", 32)
				right := signalFrame(t, other, 1)
				pixels := image.NewNRGBA(image.Rect(0, 0, 1200, 600))
				draw.Draw(pixels, image.Rect(0, 0, 600, 600), frame.NRGBA, image.Point{}, draw.Src)
				draw.Draw(pixels, image.Rect(600, 0, 1200, 600), right.NRGBA, image.Point{}, draw.Src)
				frame.NRGBA = pixels
			}
			reader := ObserveSignals(&queuedFrames{frames: []*desktop.CapturedFrame{frame}, end: end})
			got, err := reader.DiscoverReady(context.Background(), expected)
			if mode == "valid" || mode == "filtered" {
				if err != nil || got != signal {
					t.Fatal(got, err)
				}
			} else {
				if err == nil || reader.RequireFreshSignal() == nil {
					t.Fatal("discovery accepted", mode)
				}
				if mode == "ambiguous" && err.Error() != "bridge.ambiguous_signal" {
					t.Fatal("did not reject ambiguity", err)
				}
			}
		})
	}
	reader := ObserveSignals(&queuedFrames{frames: []*desktop.CapturedFrame{signalFrame(t, signal, 1)}, end: end})
	if _, err := reader.WaitForSignal(context.Background(), want); err == nil {
		t.Fatal("discovery weakened normal receipt matching")
	}
}
