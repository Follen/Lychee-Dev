package testkit

import (
	"context"
	"testing"

	"github.com/follenfang/lycheedev/internal/luals"
	"github.com/follenfang/lycheedev/internal/vault"
)

// SerializeSourceBroker coordinates integrations in different test binaries:
// the real broker admits only one owner per logon, even for different homes.
// Register this before broker cleanup so that cleanup runs before gate release.
// The separate test key does not acquire or change production broker ownership.
func SerializeSourceBroker(t testing.TB, home string) {
	t.Helper()
	gate, err := sourceBrokerTestGate(t.Context(), home)
	if err != nil {
		t.Fatalf("acquire shared broker test gate: %v", err)
	}
	t.Cleanup(func() {
		if err := gate.Close(); err != nil {
			t.Errorf("release shared broker test gate: %v", err)
		}
	})
}

func sourceBrokerTestGate(ctx context.Context, home string) (*vault.Lease, error) {
	_, _, scope, err := luals.BrokerScope(home)
	if err != nil {
		return nil, err
	}
	return vault.AcquireLease(ctx, scope, "source-broker-integration-test.v1")
}
