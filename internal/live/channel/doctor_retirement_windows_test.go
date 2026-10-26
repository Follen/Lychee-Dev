//go:build windows && amd64

package channel

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/delivery"
)

func TestDoctorRetirementKeepsDamagedSlotsForResetArchive(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), time.Minute)
	defer cancel()
	d, _, peer := retirementFixture(t)
	e, proof := d.State.Transaction.Envelope, peer.proof
	parent := t.TempDir()
	n := &Native{Target: retirementTarget(), Parent: parent, Version: proof.Current.Release, Consumer: "123/456", Guard: func(ctx context.Context) error { return ctx.Err() }}
	pool := delivery.SlotPool{Schema: "lycheedev.slots.v3", State: "ready", Version: n.Version, Files: make([]delivery.SlotFile, bridge.SlotCount)}
	for i := range pool.Files {
		pool.Files[i].PayloadHash = strings.Repeat("1", 64)
	}
	pool.Files[e.Index-1].Nonce, pool.Files[e.Index-1].Runtime, pool.Files[e.Index-1].Consumer = e.Nonce, e.Runtime, n.Consumer
	manifest, _ := json.Marshal(pool)
	marker := filepath.Join(parent, ".lycheedev-slots.json")
	if err := os.WriteFile(marker, manifest, 0600); err != nil {
		t.Fatal(err)
	}
	directory := delivery.SlotDirectory(parent, e.Index)
	if err := os.Mkdir(directory, 0700); err != nil {
		t.Fatal(err)
	}
	payload := []byte("damaged unknown original payload\x00\xff")
	path := filepath.Join(directory, "Payload.lua")
	if err := os.WriteFile(path, payload, 0600); err != nil {
		t.Fatal(err)
	}
	if err := n.RetireReplacedRuntime(ctx, e, proof); !errors.Is(err, delivery.ErrInstallation) {
		t.Fatalf("ordinary retirement lost its payload check: %v", err)
	}
	n.resetSlotsOnClose = true
	if err := n.RetireReplacedRuntime(ctx, e, proof); err != nil {
		t.Fatal(err)
	}
	for file, want := range map[string][]byte{marker: manifest, path: payload} {
		got, err := os.ReadFile(file)
		if err != nil || !bytes.Equal(got, want) {
			t.Fatalf("reset archive source changed: %s %v", file, err)
		}
	}
	wrong := *proof
	wrong.ProcessStartedAt++
	if err := n.RetireReplacedRuntime(ctx, e, &wrong); err == nil {
		t.Fatal("reset bypassed process identity")
	}
	guardErr := errors.New("guard failed")
	n.Guard = func(context.Context) error { return guardErr }
	if err := n.RetireReplacedRuntime(ctx, e, proof); !errors.Is(err, guardErr) {
		t.Fatal("reset bypassed guard", err)
	}
}
