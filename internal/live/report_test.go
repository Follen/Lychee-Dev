package live

import (
	"context"
	"encoding/json"
	"fmt"
	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/vault"
	"github.com/makiuchi-d/gozxing"
	"github.com/makiuchi-d/gozxing/qrcode"
	"hash/adler32"
	"image"
	"image/color"
	"io"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"
)

func TestOperationReportPersistsIntentBeforeAcknowledgement(t *testing.T) {
	ctx := context.Background()
	root := filepath.Join(t.TempDir(), "workspace")
	store, err := vault.Initialize(ctx, root)
	if err != nil {
		t.Fatal(err)
	}
	metadata, err := store.OpenMetadata(ctx)
	if err != nil {
		t.Fatal(err)
	}
	defer metadata.Close()
	expected := bridge.SignalExpectation{Kind: "reported", Release: "2.0.0-dev", SessionNonce: "session", RequestID: "request", Character: "paladin", Realm: "realm", Product: "retail", Build: "12.1.0.69875"}
	raw, _ := json.Marshal(ReportIntent{Schema: "lycheedev.report-intent.v1", Expected: expected})
	book := journal.OpenBook(metadata)
	record, err := book.BeginWork(ctx, journal.WorkIntent{Kind: "probe", Resource: "window/test", Session: "session", Snapshot: "PIN-test", Request: raw})
	if err != nil {
		t.Fatal(err)
	}
	if _, err := RequestReportAcknowledgement(ctx, root, record.OperationID); err == nil {
		t.Fatal("premature ACK allowed")
	}
	for _, stage := range []string{"load_requested", "loaded", "dispatch_requested", "reported", "flush_requested", "persisted"} {
		err := book.AdvanceStage(ctx, journal.StageChange{OperationID: record.OperationID, ExpectedGeneration: record.Generation, ExpectedStage: record.Stage, Stage: stage, Status: "running", Observation: json.RawMessage(`null`)})
		if err != nil {
			t.Fatal(err)
		}
		record, err = book.InspectWork(ctx, record.OperationID)
		if err != nil {
			t.Fatal(err)
		}
	}
	if _, err := ArchiveOperationReport(ctx, root, record.OperationID, strings.NewReader("invalid")); err == nil {
		t.Fatal("invalid state accepted")
	}
	still, err := book.InspectWork(ctx, record.OperationID)
	if err != nil || still.Generation != record.Generation {
		t.Fatal("failed verification advanced operation", err)
	}
	body := `{"answer":42}`
	receipt, _ := json.Marshal(bridge.Signal{Schema: "lycheedev.signal.v1", Release: expected.Release, Kind: expected.Kind, SessionNonce: expected.SessionNonce, RequestID: expected.RequestID, Character: expected.Character, Realm: expected.Realm, Product: expected.Product, Build: expected.Build, Sequence: 1, ReportBytes: uint32(len(body)), ReportAdler32: fmt.Sprintf("%08x", adler32.Checksum([]byte(body)))})
	saved := fmt.Sprintf(`LycheeToolkitDB = { schema = 1, reports = { request = { receipt = %q, body = %q } } }`, receipt, body)
	pair, err := ArchiveOperationReport(ctx, root, record.OperationID, strings.NewReader(saved))
	if err != nil {
		t.Fatal(err)
	}
	if pair.Body.ID == "" || pair.Receipt.ID == "" {
		t.Fatal("missing durable evidence")
	}
	if err := metadata.Close(); err != nil {
		t.Fatal(err)
	}
	// Competing resumptions can both read verified, but only one may persist the
	// generation-checked ACK intent and receive sendable bytes.
	var group sync.WaitGroup
	results := make(chan bool, 2)
	for i := 0; i < 2; i++ {
		group.Add(1)
		go func() {
			defer group.Done()
			report, err := RequestReportAcknowledgement(ctx, root, record.OperationID)
			results <- err == nil && string(report.Body) == body && string(report.ReceiptBytes) == string(receipt)
		}()
	}
	group.Wait()
	close(results)
	successes := 0
	for success := range results {
		if success {
			successes++
		}
	}
	if successes != 1 {
		t.Fatalf("sendable ACK results = %d", successes)
	}
	final, err := InspectOperation(ctx, root, record.OperationID)
	if err != nil || final.Stage != "ack_requested" || final.Status != "running" {
		t.Fatalf("wrong durable stage: %+v %v", final, err)
	}
	if _, err := RequestReportAcknowledgement(ctx, root, record.OperationID); err == nil {
		t.Fatal("unresolved ACK replay allowed")
	}
	var ack bridge.Signal
	if err := json.Unmarshal(receipt, &ack); err != nil {
		t.Fatal(err)
	}
	ack.Kind = "acknowledged"
	ack.Sequence++
	for _, mode := range []string{"missing", "stale", "foreign", "checksum"} {
		t.Run(mode, func(t *testing.T) {
			candidate := ack
			feed := &ackFrames{}
			switch mode {
			case "stale":
				candidate.Sequence--
			case "foreign":
				candidate.SessionNonce = "other"
			case "checksum":
				candidate.ReportAdler32 = "ffffffff"
			}
			if mode != "missing" {
				feed.frame = makeAckFrame(t, candidate)
			}
			if _, err := ObserveReportAcknowledgement(ctx, root, record.OperationID, bridge.ObserveSignals(feed)); err == nil {
				t.Fatal("invalid ACK advanced operation")
			}
			unchanged, err := InspectOperation(ctx, root, record.OperationID)
			if err != nil || unchanged.Generation != final.Generation {
				t.Fatal("invalid ACK changed stage", err)
			}
		})
	}
	ackCapture, err := ObserveReportAcknowledgement(ctx, root, record.OperationID, bridge.ObserveSignals(&ackFrames{frame: makeAckFrame(t, ack)}))
	if err != nil || ackCapture.ID == "" {
		t.Fatalf("valid ACK not recorded: %v", err)
	}
	confirmed, err := InspectOperation(ctx, root, record.OperationID)
	if err != nil || confirmed.Stage != "acknowledged" || confirmed.Status != "running" {
		t.Fatalf("ACK became cleanup or failed: %+v %v", confirmed, err)
	}
	if _, err := ObserveReportAcknowledgement(ctx, root, record.OperationID, bridge.ObserveSignals(&ackFrames{})); err == nil {
		t.Fatal("duplicate ACK accepted")
	}
}

type ackFrames struct{ frame *desktop.CapturedFrame }

func (f *ackFrames) Next(ctx context.Context) (*desktop.CapturedFrame, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if f.frame == nil {
		return nil, io.EOF
	}
	frame := f.frame
	f.frame = nil
	return frame, nil
}
func makeAckFrame(t *testing.T, signal bridge.Signal) *desktop.CapturedFrame {
	t.Helper()
	raw, err := json.Marshal(signal)
	if err != nil {
		t.Fatal(err)
	}
	bitmap, err := qrcode.NewQRCodeWriter().Encode(string(raw), gozxing.BarcodeFormat_QR_CODE, 600, 600, nil)
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
			pixels.SetNRGBA(x, y, color.NRGBA{R: shade, G: shade, B: shade, A: 255})
		}
	}
	return &desktop.CapturedFrame{NRGBA: pixels, SystemTicks: 1, ObservedAt: time.Now()}
}
