//go:build windows && amd64

package channel

import (
	"context"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/live/memory"
)

func TestNativeMailboxRequiredWithoutScanFallback(t *testing.T) {
	n := &Native{Process: &memory.Process{}}
	found, err := n.Find(context.Background(), memory.Selector{}, true)
	if err == nil || !strings.Contains(err.Error(), "live.channel_mailbox_required") || len(found.Records) != 0 {
		t.Fatal(found, err)
	}
}
