package live

import (
	"context"
	"errors"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"testing"
)

func TestAbandonRefusesPreparedProbe(t *testing.T) {
	root, client, book, record, _ := probeOperationFixture(t)
	before, err := book.InspectWork(context.Background(), record.OperationID)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := Abandon(context.Background(), root, record.OperationID); !errors.Is(err, journal.ErrTransition) {
		t.Fatal(err)
	}
	after, err := book.InspectWork(context.Background(), record.OperationID)
	if err != nil || before.Generation != after.Generation {
		t.Fatal("refused abandonment changed work", err)
	}
	if _, occupied, err := journal.InspectWindowOwner(context.Background(), client+"/Interface/AddOns", record.Intent.Resource); err != nil || !occupied {
		t.Fatal("refused abandonment released owner", err)
	}
}
