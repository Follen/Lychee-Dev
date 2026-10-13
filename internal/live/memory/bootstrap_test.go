package memory

import (
	"context"
	"encoding/binary"
	"errors"
	"github.com/follenfang/lycheedev/internal/bridge"
	"testing"
)

func TestBootstrapRemainsReadableWithoutBusinessMailbox(t *testing.T) {
	f := newMailboxFixture(t, 0x140000000, 0x200000000)
	// A later module failed before publishing the business mailbox.
	clear(f.bytes(f.nodes["namespace.Mailbox"], 16))
	r := f.reader()
	boot, err := r.Lookup(context.Background(), f.selectors["bootstrap"])
	if err != nil || len(boot.Records) != 1 || f.regions != 0 {
		t.Fatalf("bootstrap required business startup: %+v %v", boot, err)
	}
	for _, name := range []string{"input", "identity"} {
		out, err := r.Lookup(context.Background(), f.selectors[name])
		if err != nil || len(out.Records) != 0 {
			t.Fatal("bootstrap granted business evidence", name, err)
		}
	}
}

func TestPreviousProtocolPublishesUpgradeEvidenceOnly(t *testing.T) {
	f := newMailboxFixture(t, 0x140000000, 0x200000000)
	binary.LittleEndian.PutUint64(f.bytes(f.nodes["box.schema"], 8), f.text([]byte(bridge.PreviousMailboxSchema)))
	for _, name := range []string{"identity", "input", "receipt", "confirmation", "body"} {
		out, err := f.reader().Lookup(context.Background(), f.selectors[name])
		if name == "identity" || name == "input" {
			if err != nil || len(out.Records) != 1 {
				t.Fatal(name, err)
			}
		} else if !errors.Is(err, ErrMailboxIncompatible) || len(out.Records) != 0 {
			t.Fatal("old business publication accepted", name, err)
		}
	}
}
