//go:build windows && amd64

package channel

import (
	"context"
	"encoding/json"
	"errors"
	"hash/adler32"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/delivery"
	"github.com/follenfang/lycheedev/internal/vault"
)

func TestPublicationBlockerIdentifiesExactReservation(t *testing.T) {
	n, reserved := publicationFixture(t)
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	reserved.Action = "prepare"
	reserved.Code = "return 'private-probe-content'"
	reserved.CodeBytes = len(reserved.Code)
	reserved.CodeChecksum = adler32.Checksum([]byte(reserved.Code))
	reserved.Budget = 5
	if err := n.Publish(ctx, reserved); err != nil {
		t.Fatal(err)
	}
	other := &Native{Parent: n.Parent, Version: n.Version, Consumer: "2/2", Guard: n.Guard}
	wanted := reserved
	wanted.Runtime, wanted.Nonce = strings.Repeat("6", 32), strings.Repeat("7", 32)
	err := other.Publish(ctx, wanted)
	var blocked *BlockedError
	if !errors.As(err, &blocked) || !errors.Is(err, ErrPublicationPending) || !errors.Is(err, ErrPending) {
		t.Fatalf("lost pending compatibility: %v", err)
	}
	b := blocked.Blocker
	if b.Kind != "slot_reservation" || b.Installation != n.Parent || b.Slot != reserved.Index || b.Consumer != n.Consumer || b.Runtime != reserved.Runtime || b.Nonce != reserved.Nonce || b.Condition != "exact_consumption_or_retirement_evidence" {
		t.Fatalf("wrong dependency: %+v", b)
	}
	data, err := json.Marshal(b)
	if err != nil {
		t.Fatal(err)
	}
	var fields map[string]json.RawMessage
	if err = json.Unmarshal(data, &fields); err != nil {
		t.Fatal(err)
	}
	if len(fields) != 7 {
		t.Fatalf("unexpected exposed fields: %s", data)
	}
	if strings.Contains(string(data), reserved.Code) || strings.Contains(blocked.Error(), reserved.Code) {
		t.Fatal("blocker exposed another request's probe code")
	}
	if other.Publication != nil {
		t.Fatal("diagnostic retained publication lease")
	}
	if err = delivery.VerifySlotPublication(ctx, n.Parent, n.Version, n.Consumer, reserved); err != nil {
		t.Fatal("diagnostic altered reservation", err)
	}
	if err = n.Consumed(ctx, reserved); err != nil {
		t.Fatal(err)
	}
	if err = other.Publish(ctx, wanted); err != nil {
		t.Fatal("exact dependency resolution did not unblock publication", err)
	}
}

func TestPublicationBlockerDistinguishesOSLeaseWait(t *testing.T) {
	n, e := publicationFixture(t)
	lease, err := vault.AcquireLease(context.Background(), filepath.Join(n.Parent, ".lycheedev-slot-locks"), "publication")
	if err != nil {
		t.Fatal(err)
	}
	defer lease.Close()
	ctx, cancel := context.WithTimeout(context.Background(), 60*time.Millisecond)
	defer cancel()
	err = n.Publish(ctx, e)
	var blocked *BlockedError
	if !errors.As(err, &blocked) || !errors.Is(err, ErrPublicationPending) || !errors.Is(err, context.DeadlineExceeded) {
		t.Fatalf("lost lease/deadline evidence: %v", err)
	}
	b := blocked.Blocker
	if b.Kind != "publication_lock" || b.Installation != n.Parent || b.Condition != "publication_lease_released" || b.Slot != 0 || b.Consumer != "" || b.Runtime != "" || b.Nonce != "" {
		t.Fatalf("invented durable reservation for OS lease wait: %+v", b)
	}
	if n.Publication != nil {
		t.Fatal("waiting caller acquired lease")
	}
}
