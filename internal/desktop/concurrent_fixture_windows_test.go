//go:build windows && amd64

package desktop

import (
	"context"
	"image"
	"os"
	"testing"
	"time"
	"unicode/utf16"

	"github.com/makiuchi-d/gozxing"
	"github.com/makiuchi-d/gozxing/qrcode"
)

// Exercise the native connect transport overlap without sending game input.
func TestBackgroundInputDuringWGCCaptureOwnWindow(t *testing.T) {
	if os.Getenv("LYCHEEDEV_TEST_DESKTOP") != "1" {
		t.Skip("requires an interactive desktop for native WGC")
	}
	const payload = "concurrent capture fixture"
	bitmap, err := qrcode.NewQRCodeWriter().Encode(payload, gozxing.BarcodeFormat_QR_CODE, 600, 600, nil)
	if err != nil {
		t.Fatal(err)
	}
	f := openFixtureWindow(t, bitmap)
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	stream, err := CaptureFrames(ctx, f.identity, image.Rect(0, 0, 600, 600))
	if err != nil {
		t.Fatal(err)
	}
	defer stream.Close()
	// Like probeIdentity, queue the complete bootstrap while capture is active,
	// then consume the optical receipt. The fixture never interprets the text.
	const command = "/dev bridge identify 0123456789abcdef0123456789abcdef"
	receipt, err := QueueBootstrapCommand(ctx, f.identity, command)
	if err != nil {
		t.Fatal(err)
	}
	if !receipt.SubmissionComplete {
		t.Fatal(receipt)
	}
	var chars []uint16
	downs, ups := 0, 0
	for consumed := 0; consumed < receipt.MessagesQueued; {
		select {
		case packet := <-f.packets:
			if packet.message == 0x102 && packet.value == 13 {
				continue
			}
			consumed++
			switch packet.message {
			case 0x100:
				downs++
			case 0x101:
				ups++
			case 0x102:
				chars = append(chars, uint16(packet.value))
			}
		case <-ctx.Done():
			t.Fatal(ctx.Err())
		}
	}
	if string(utf16.Decode(chars)) != command || downs != 2 || ups != 2 {
		t.Fatalf("decoded=%q down=%d up=%d", string(utf16.Decode(chars)), downs, ups)
	}
	for {
		frame, err := stream.Next(ctx)
		if err != nil {
			t.Fatal(err)
		}
		symbols, err := DecodeSymbols(frame)
		if err != nil {
			t.Fatal(err)
		}
		for _, symbol := range symbols {
			if symbol == payload {
				return
			}
		}
	}
}
