package command

import (
	"bytes"
	"context"
	"encoding/binary"
	"errors"
	"github.com/follenfang/lycheedev/internal/luals"
	"testing"
)

func TestSourceSessionOptInContract(t *testing.T) {
	opts, err := parseOptions([]string{"source", "refs", "--symbol", "Foo", "--session-reuse"})
	if err != nil || !opts.sessionReuse {
		t.Fatalf("session opt-in: %+v %v", opts, err)
	}
	for _, route := range []string{"status", "close"} {
		if _, err = parseOptions([]string{"source", "session", route}); err != nil {
			t.Fatal(err)
		}
	}
	for _, args := range [][]string{{"source", "query", "Foo", "--session-reuse"}, {"doctor", "--session-reuse"}} {
		if _, err = parseOptions(args); err == nil {
			t.Fatalf("accepted unrelated opt-in: %v", args)
		}
	}
	if code := SourceBrokerMain(context.Background(), []string{"--internal-luals-broker", "arbitrary"}); code != 2 {
		t.Fatalf("internal entry accepted untrusted arguments: %d", code)
	}
}
func TestBrokerFramesBoundedAndStrict(t *testing.T) {
	var raw bytes.Buffer
	if err := writeBrokerFrame(&raw, brokerRequest{Wire: luals.BrokerWire}, brokerRequestBytes); err != nil {
		t.Fatal(err)
	}
	var request brokerRequest
	if err := readBrokerFrame(&raw, &request, brokerRequestBytes); err != nil {
		t.Fatal(err)
	}
	var header [4]byte
	binary.LittleEndian.PutUint32(header[:], brokerRequestBytes+1)
	if err := readBrokerFrame(bytes.NewReader(header[:]), &request, brokerRequestBytes); !errors.Is(err, luals.ErrBudget) {
		t.Fatalf("oversized request: %v", err)
	}
	raw.Reset()
	if err := writeBrokerFrame(&raw, map[string]any{"wire": luals.BrokerWire, "executable": "untrusted.exe"}, brokerRequestBytes); err != nil {
		t.Fatal(err)
	}
	if err := readBrokerFrame(&raw, &request, brokerRequestBytes); err == nil {
		t.Fatal("broker accepted executable injection field")
	}
}
func TestBrokerQueueCancellationRemovesOnlyItsOwnPendingTask(t *testing.T) {
	queue := newSourceBrokerQueue()
	tasks := make([]*sourceBrokerTask, 16)
	for i := range tasks {
		tasks[i] = &sourceBrokerTask{}
		if !queue.submit(tasks[i]) {
			t.Fatalf("queue rejected entry%d", i)
		}
	}
	if queue.submit(&sourceBrokerTask{}) {
		t.Fatal("queue exceeded sixteen pending entries")
	}
	queue.remove(tasks[7])
	replacement := &sourceBrokerTask{}
	if !queue.submit(replacement) {
		t.Fatal("cancelled task still occupied queue capacity")
	}
	for i := range tasks {
		if i == 7 {
			continue
		}
		if got := queue.next(); got != tasks[i] {
			t.Fatalf("cancel changed another caller order at%d", i)
		}
	}
	if got := queue.next(); got != replacement {
		t.Fatal("replacement not queued after other callers")
	}
	if got := queue.next(); got != nil {
		t.Fatal("queue did not drain")
	}
}
