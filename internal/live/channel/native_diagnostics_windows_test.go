//go:build windows && amd64

package channel

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/live/memory"
)

type diagnosticSource struct{ verifies int }

type legacyActivationMailbox struct{ record memory.Record }

func (m *legacyActivationMailbox) Lookup(ctx context.Context, selector memory.Selector) (memory.LookupResult, error) {
	if selector.Kind != bridge.MemoryIdentity {
		return memory.LookupResult{}, errors.New("unexpected input observation")
	}
	return memory.LookupResult{Records: []memory.Record{m.record}, Coverage: memory.Coverage{Complete: true}}, ctx.Err()
}
func (*legacyActivationMailbox) Binding() memory.LuaRootBinding { return memory.LuaRootBinding{} }

func TestNativePublishedLegacyIdentityKeepsManualActivationClassification(t *testing.T) {
	n := &Native{Version: "3.1.2", Mailbox: &legacyActivationMailbox{record: publishedLegacyIdentityRecord(t)}}
	identities, coverage, err := n.Discover(context.Background(), "", "")
	if !errors.Is(err, memory.ErrMailboxIncompatible) || len(identities) != 0 || !coverage.Complete {
		t.Fatalf("published legacy runtime cannot reach the manual-activation branch: identities=%d coverage=%+v err=%v", len(identities), coverage, err)
	}
}

func (s *diagnosticSource) Lookup(ctx context.Context, _ memory.Selector) (memory.LookupResult, error) {
	s.verifies++
	return memory.LookupResult{}, ctx.Err()
}
func (*diagnosticSource) Binding() memory.LuaRootBinding { return memory.LuaRootBinding{} }

func (s *diagnosticSource) Verify(ctx context.Context) error { s.verifies++; return ctx.Err() }
func (*diagnosticSource) Read(context.Context, uint64, []byte) (int, error) {
	return 0, errors.New("unexpected read")
}

func TestFindDefersTraceIOUntilClose(t *testing.T) {
	root := t.TempDir()
	sink := filepath.Join(root, "blocked")
	if err := os.WriteFile(sink, []byte("file blocks directory creation"), 0600); err != nil {
		t.Fatal(err)
	}
	n := &Native{TraceDir: filepath.Join(sink, "scans"), Mailbox: &diagnosticSource{}}
	source := &diagnosticSource{}
	n.Mailbox = source
	for i := 0; i < 3; i++ {
		if _, err := n.Find(context.Background(), memory.Selector{}, true); err != nil {
			t.Fatal("diagnostic sink blocked the lookup", err)
		}
	}

	if len(n.traces) != 3 || n.traceSequence != 3 {
		t.Fatal("lost buffered evidence")
	}
	if err := n.Close(); !errors.Is(err, ErrTraceFlush) {
		t.Fatal("flush error hidden", err)
	}
	if len(n.traces) != 3 {
		t.Fatal("failed diagnostics discarded")
	}
	// Exact same buffered evidence can be flushed after the sink is repaired.
	if err := os.Remove(sink); err != nil {
		t.Fatal(err)
	}
	if err := n.Close(); err != nil {
		t.Fatal(err)
	}
	if len(n.traces) != 0 || n.traceSequence != 3 {
		t.Fatal("flush renewed observation capacity")
	}
	for _, name := range []string{"001.json", "002.json", "003.json"} {
		if _, err := os.Stat(filepath.Join(n.TraceDir, name)); err != nil {
			t.Fatal(err)
		}
	}

}

func TestTraceFlushIsBoundedAndRetainsOnlyUnwrittenEvidence(t *testing.T) {
	n := &Native{TraceDir: t.TempDir(), Mailbox: &diagnosticSource{}}
	source := &diagnosticSource{}
	n.Mailbox = source
	for i := 0; i < 3; i++ {
		if _, err := n.Find(context.Background(), memory.Selector{}, true); err != nil {
			t.Fatal(err)
		}
	}
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	calls := 0
	writer := func(ctx context.Context, path string, value any) error {
		calls++
		if calls == 2 {
			cancel()
			return ctx.Err()
		}
		return writeProjectJSON(ctx, path, value)
	}
	if err := n.flushTraces(ctx, writer); !errors.Is(err, context.Canceled) || !errors.Is(err, ErrTraceFlush) {
		t.Fatal(err)
	}
	if calls != 2 || len(n.traces) != 2 || n.traces[0].sequence != 2 {
		t.Fatal("incorrect retry boundary")
	}
	if err := n.flushTraces(context.Background(), writeProjectJSON); err != nil {
		t.Fatal(err)
	}
	if n.traces != nil {
		t.Fatal("trace buffer retained after flush")
	}
	for i := 3; i < observationLimit; i++ {
		if _, err := n.Find(context.Background(), memory.Selector{}, true); err != nil {
			t.Fatal(err)
		}
	}
	before := source.verifies
	for i := 0; i < 20; i++ {
		if _, err := n.Find(context.Background(), memory.Selector{}, true); err != nil {
			t.Fatal("diagnostic cap blocked observation", err)
		}
	}
	if n.tracesOmitted != 20 || source.verifies <= before || len(n.Lookups) != observationLimit || len(n.traces) > observationLimit {
		t.Fatal("lookup stopped or diagnostics grew without bound")
	}

	if err := n.Close(); err != nil {
		t.Fatal(err)
	}
	var summary struct {
		Observations, Retained, Omitted int
		Truncated                       bool
	}
	if err := readProjectJSON(filepath.Join(n.TraceDir, "summary.json"), &summary, 4096); err != nil {
		t.Fatal(err)
	}
	if !summary.Truncated || summary.Omitted != 20 || summary.Retained != observationLimit || summary.Observations != observationLimit+20 {
		t.Fatalf("bad truncation evidence: %+v", summary)
	}

}

func TestCancelledLookupStillFlushesAtClose(t *testing.T) {
	n := &Native{TraceDir: filepath.Join(t.TempDir(), "scans"), Mailbox: &diagnosticSource{}}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := n.Find(ctx, memory.Selector{}, true); !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
	if err := n.Close(); err != nil {
		t.Fatal("caller cancellation discarded diagnostics", err)
	}
	var trace lookupTrace
	if err := readProjectJSON(filepath.Join(n.TraceDir, "001.json"), &trace, 1<<20); err != nil {
		t.Fatal(err)
	}
	if trace.Matches != 0 || trace.Lookup.Records != nil {
		t.Fatal("unexpected trace body")
	}
}

func TestSlowTraceWriterRunsOnlyAtFlushAndObeysSharedDeadline(t *testing.T) {
	n := &Native{TraceDir: t.TempDir(), Mailbox: &diagnosticSource{}}
	for i := 0; i < 2; i++ {
		if _, err := n.Find(context.Background(), memory.Selector{}, true); err != nil {
			t.Fatal(err)
		}
	}
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Millisecond)
	defer cancel()
	calls := 0
	err := n.flushTraces(ctx, func(ctx context.Context, _ string, _ any) error { calls++; <-ctx.Done(); return ctx.Err() })
	if !errors.Is(err, context.DeadlineExceeded) || !errors.Is(err, ErrTraceFlush) || calls != 1 || len(n.traces) != 2 {
		t.Fatal("per-item timeout multiplied total budget", err, calls)
	}
}

func TestActivationFinalizesAuthorityAndReleasesDriverBeforeDiagnostics(t *testing.T) {
	p, d, meta, parent := projectFixture(t)
	lease, err := journal.LockBootstrapWindow(context.Background(), parent, meta.Owner)
	if err != nil {
		t.Fatal(err)
	}
	defer lease.Close()
	a := activation{Schema: "lycheedev.channel-activation.v3", Request: "activation", Phase: "prepared", Budget: NewDurableBudget(time.Now().Add(-time.Second), DefaultRecoveryBudget, false)}
	if err := writeProjectJSON(context.Background(), p.activationPath(d.State.ID), a); err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithCancelCause(context.Background())
	defer cancel(nil)
	traceFailure := errors.New("injected trace flush failure")
	order := []string{}
	err = finishActivationResources(func() error {
		order = append(order, "budget")
		if other, e := journal.LockBootstrapWindow(context.Background(), parent, meta.Owner); e == nil {
			other.Close()
			t.Fatal("budget finalization lost exclusive driver")
		} else if !errors.Is(e, journal.ErrBusy) {
			t.Fatal(e)
		}
		return p.finishActivationBudget(ctx, d.State.ID, &a)
	}, func() error { order = append(order, "driver"); return lease.Close() }, func() error {
		order = append(order, "diagnostics")
		other, e := journal.LockBootstrapWindow(context.Background(), parent, meta.Owner)
		if e != nil {
			t.Fatal("diagnostics held window driver", e)
		}
		defer other.Close()
		var durable activation
		if e = readProjectJSON(p.activationPath(d.State.ID), &durable, 16384); e != nil {
			t.Fatal(e)
		}
		if durable.Budget.LastObservedMS != a.Budget.LastObservedMS || durable.Budget.Exhausted {
			t.Fatal("authority budget not finalized before diagnostics")
		}
		// A deadline firing during non-authoritative diagnostic IO must not
		// retroactively change the already-finalized activation observation.
		cancel(ErrBudgetExhausted)
		return traceFailure
	})
	if !errors.Is(err, traceFailure) || strings.Join(order, ",") != "budget,driver,diagnostics" {
		t.Fatal(order, err)
	}
	var durable activation
	if e := readProjectJSON(p.activationPath(d.State.ID), &durable, 16384); e != nil || durable.Budget.Exhausted {
		t.Fatal("diagnostic deadline spent authority budget", e)
	}
}

func TestActivationFinalizerRetainsEveryCleanupFailure(t *testing.T) {
	budgetFailure := errors.New("budget write failed")
	driverFailure := errors.New("driver release failed")
	traceFailure := errors.New("trace flush failed")
	err := finishActivationResources(func() error { return budgetFailure }, func() error { return driverFailure }, func() error { return traceFailure })
	for _, cause := range []error{budgetFailure, driverFailure, traceFailure} {
		if !errors.Is(err, cause) {
			t.Fatal("cleanup error hidden", cause, err)
		}
	}
	if err := finishActivationResources(nil, nil, nil); err != nil {
		t.Fatal(err)
	}
}
