//go:build windows

package desktop

import (
	"context"
	"errors"
	"fmt"
	"image"
	"testing"
)

func TestPreparedInputGuardOwnWindow(t *testing.T) {
	for _, stop := range []int{1, 2, 4, 0} {
		t.Run(fmt.Sprint(stop), func(t *testing.T) {
			f := openFixtureWindow(t, image.NewNRGBA(image.Rect(0, 0, 120, 80)))
			checks, prepared := 0, 0
			stopped := errors.New("fixture.guard_changed")
			receipt, err := QueuePreparedCommand(context.Background(), f.identity, func(context.Context) (string, error) { prepared++; return "/dev status", nil }, func(context.Context) error {
				checks++
				if checks == stop {
					return stopped
				}
				return nil
			})
			if stop == 0 {
				if err != nil || !receipt.SubmissionComplete || prepared != 1 || checks != receipt.MessagesQueued+1 {
					t.Fatalf("submitted: %+v %d %d %v", receipt, prepared, checks, err)
				}
				return
			}
			if !errors.Is(err, stopped) || receipt.SubmissionComplete {
				t.Fatalf("guard: %+v %v", receipt, err)
			}
			want := stop - 2
			if want < 0 {
				want = 0
			}
			if receipt.MessagesQueued != want {
				t.Fatalf("sent after guard failed: %+v", receipt)
			}
			if stop == 1 && prepared != 0 {
				t.Fatal("prepared without initial ownership")
			}
		})
	}
}
