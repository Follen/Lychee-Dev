package live

import (
	"context"
	"errors"
	"github.com/follenfang/lycheedev/internal/desktop"
	"image"
	"testing"
)

func TestResumeLiveRejectsChangedWindowBeforeCaptureOrInput(t *testing.T) {
	ctx := context.Background()
	root, _, book, record, _ := probeOperationFixture(t)
	changed := errors.New("fixture.process_restarted")
	captures, sends := 0, 0
	result, err := resumeLiveOperation(ctx, root, record.OperationID, image.Rectangle{}, func(context.Context, ClientWindow, image.Rectangle) (sessionFrames, error) {
		captures++
		return nil, errors.New("unexpected capture")
	}, func(context.Context, ClientWindow) error { return changed }, func(context.Context, desktop.WindowIdentity, func(context.Context) (string, error), func(context.Context) error) (desktop.InputReceipt, error) {
		sends++
		return desktop.InputReceipt{}, errors.New("unexpected input")
	})
	if !errors.Is(err, changed) || captures != 0 || sends != 0 || result.OperationID != record.OperationID {
		t.Fatal(result, err, captures, sends)
	}
	current, err := book.InspectWork(ctx, record.OperationID)
	if err != nil || current.Generation != record.Generation {
		t.Fatal("failed resume changed work", err)
	}
}
