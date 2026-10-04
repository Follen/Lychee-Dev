//go:build windows && amd64

package memory

import (
	"context"
	"strings"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/live/duplex"
)

func TestStoppedHelperRejectsDirectOnlyRetailProfile(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	request := StoppedPublicationRequest{
		ExecutableSHA256: "d41f11de411f6fdb280a7c1c6380ba3d1b17614f03e5aca8fabce2cc715cd7dd",
		Build: "12.1.0.69933", Product: "retail",
	}
	out, _, stop, err := PublishStoppedDuplexRow(ctx, request)
	if out.State != duplex.NoWrite || stop.Attached || err == nil || !strings.Contains(err.Error(), "writer_profile_unverified") {
		t.Fatalf("direct-only profile entered debugger helper: outcome=%+v stop=%+v error=%v", out, stop, err)
	}
}
