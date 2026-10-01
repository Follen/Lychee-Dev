//go:build windows && amd64

package channel

import (
	"context"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/live/memory"
)

type forbiddenMailboxScan struct{ t *testing.T }

func (s forbiddenMailboxScan) Verify(context.Context) error { return nil }
func (s forbiddenMailboxScan) Regions(context.Context) ([]memory.Region, error) {
	s.t.Fatal("native communication enumerated heap regions")
	return nil, nil
}
func (s forbiddenMailboxScan) Read(context.Context, uint64, []byte) (int, error) {
	s.t.Fatal("missing mailbox triggered a scan or retained address read")
	return 0, nil
}

func TestNativeMailboxRequiredWithoutScanFallback(t *testing.T) {
	n := &Native{Process: &memory.Process{}}
	for _, nearby := range []bool{false, true} {
		found, err := n.findPath(context.Background(), forbiddenMailboxScan{t}, memory.Selector{}, true, nearby)
		if err == nil || !strings.Contains(err.Error(), "live.channel_mailbox_required") || len(found.Records) != 0 {
			t.Fatal("native path without mailbox was accepted", found, err)
		}
	}
}
