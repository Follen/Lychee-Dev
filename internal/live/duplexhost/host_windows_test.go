//go:build windows && amd64

package duplexhost

import (
	"context"
	"encoding/json"
	"errors"
	"math"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/live/duplex"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/live/memory"
	"github.com/follenfang/lycheedev/internal/vault"
)

var hostIdentity = duplex.Identity{Runtime: "11111111111111111111111111111111", Arena: "22222222222222222222222222222222", Session: "33333333333333333333333333333333", Owner: "44444444444444444444444444444444", ActorBinding: "55555555555555555555555555555555", Fence: 1}
var errNoMemoryWrite = errors.New("fixture has no writable numeric arena")

type readFixture struct {
	pages [][]byte
	reads int
}

func (f *readFixture) ReadDuplexString(_ context.Context, path []memory.DuplexPath, maximum int) ([]byte, error) {
	f.reads++
	if len(path) != 3 || path[0].Name != "sendbox" || path[1].Name != "resultPages" || path[2].Index < 1 || path[2].Index > len(f.pages) {
		return nil, errors.New("unexpected read path")
	}
	p := f.pages[path[2].Index-1]
	if len(p) > maximum {
		return nil, errors.New("read exceeded maximum")
	}
	return append([]byte(nil), p...), nil
}
func (f *readFixture) ResolveDuplexArray(context.Context, []memory.DuplexPath, string, string, int) (*memory.DuplexArray, error) {
	return nil, errNoMemoryWrite
}
func (f *readFixture) Binding() memory.LuaRootBinding { return memory.LuaRootBinding{} }
func resultFixture(source []byte) (duplex.ResultManifest, [][]byte) {
	m := duplex.ResultManifest{RequestID: "66666666666666666666666666666666", RequestSHA256: digestBytes([]byte("source")), State: "success", SHA256: digestBytes(source), Bytes: uint32(len(source))}
	var pages [][]byte
	for i := 0; i < len(source); i += 16384 {
		pages = append(pages, append([]byte(nil), source[i:min(i+16384, len(source))]...))
	}
	if len(pages) == 0 {
		pages = [][]byte{{}}
	}
	m.Pages = uint32(len(pages))
	for _, p := range pages {
		m.PageSHA256 = append(m.PageSHA256, digestBytes(p))
	}
	return m, pages
}

func TestReadResultExactPagesAndImmutableManifest(t *testing.T) {
	ctx := context.Background()
	for _, size := range []int{0, 1, 16384, 16385, duplex.MaxResultBytes} {
		t.Run(string(rune(size)), func(t *testing.T) {
			source := make([]byte, size)
			for i := range source {
				source[i] = byte(i)
			}
			manifest, pages := resultFixture(source)
			f := &readFixture{pages: pages}
			box := duplex.Sendbox{Identity: hostIdentity, Terminal: &manifest}
			n := &Native{Mailbox: f, observe: func(context.Context) (duplex.Sendbox, error) { return box, nil }}
			p, e := n.ReadResult(ctx, manifest)
			if e != nil || digestBytes(p) != manifest.SHA256 {
				t.Fatal(e)
			}
			if f.reads != len(pages) {
				t.Fatal("unexpected page reads")
			}
			f.pages[0] = append([]byte(nil), f.pages[0]...)
			f.pages[0] = append(f.pages[0], 0)
			if _, e = n.ReadResult(ctx, manifest); e == nil {
				t.Fatal("accepted changed page length")
			}
		})
	}
	manifest, pages := resultFixture([]byte("result"))
	calls := 0
	n := &Native{Mailbox: &readFixture{pages: pages}, observe: func(context.Context) (duplex.Sendbox, error) {
		calls++
		m := manifest
		if calls > 1 {
			m.RequestSHA256 = digestBytes([]byte("different-source"))
		}
		return duplex.Sendbox{Identity: hostIdentity, Terminal: &m}, nil
	}}
	if _, e := n.ReadResult(ctx, manifest); !errors.Is(e, duplex.ErrIdentity) {
		t.Fatal("accepted changed complete manifest", e)
	}
}

func TestWriterLanesIndependentAndDrainFailClosed(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	n := &Native{}
	n.Target.Client.Directory = t.TempDir()
	n.Target.Window.ProcessID = 7
	n.Target.Window.ProcessStartedAt = 9
	busy, e := vault.AcquireLease(ctx, n.scope(), n.resource("control-cancel"))
	if e != nil {
		t.Fatal(e)
	}
	defer busy.Close()
	closeLane, e := vault.TryAcquireLease(ctx, n.scope(), n.resource("control-close"))
	if e != nil {
		t.Fatal("cancel blocked independent close", e)
	}
	closeLane.Close()
	if release, e := n.WritersDrained(ctx); e == nil {
		release()
		t.Fatal("drain accepted active cancel writer")
	}
	data, e := vault.TryAcquireLease(ctx, n.scope(), n.resource("data"))
	if e != nil {
		t.Fatal("failed drain retained earlier data lock", e)
	}
	data.Close()
	busy.Close()
	release, e := n.WritersDrained(ctx)
	if e != nil {
		t.Fatal(e)
	}
	for _, lane := range writeLanes {
		lease, e := vault.TryAcquireLease(ctx, n.scope(), n.resource(lane))
		if e == nil {
			lease.Close()
			t.Fatalf("drain omitted %s", lane)
		}
	}
	release()
}

func TestRepairDoesNotReacquireOwnLane(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	dir := t.TempDir()
	store := duplex.NewFileStore(dir)
	source := []byte("return 42")
	created := uint64(time.Now().UnixMilli())
	frames, e := duplex.NewFrames(hostIdentity, "66666666666666666666666666666666", 1, 1, created, 1000, source)
	if e != nil {
		t.Fatal(e)
	}
	if e = store.Update(ctx, func(st *duplex.State) error {
		st.Identity = hostIdentity
		st.Bound = true
		st.RequestSequence = 1
		st.Active = &duplex.ActiveRequest{RequestID: frames[0].Header.RequestID, Digest: frames[0].Header.RequestSHA256, Sequence: 1, Attempt: 1, Created: created, Budget: 1000, TransferDeadline: created + 600000, Source: source, Frames: frames}
		return nil
	}); e != nil {
		t.Fatal(e)
	}
	box := duplex.Sendbox{Identity: hostIdentity, Schema: "lycheedev.mailbox.v1", LayoutID: "single-data-row-v1", Phase: "idle", TransportReady: true, ControlReady: true, Heartbeat: 1, StatusSequence: 1}
	box.Arena = "77777777777777777777777777777777"
	box.Repair = &duplex.RepairProof{PreviousArena: hostIdentity.Arena, NewArena: box.Arena, Challenge: "88888888888888888888888888888888", RequestID: frames[0].Header.RequestID, RequestSHA256: frames[0].Header.RequestSHA256, NotStarted: true, LedgerRetained: true}
	n := &Native{Mailbox: &readFixture{}, observe: func(context.Context) (duplex.Sendbox, error) { return box, nil }}
	n.Target.Client.Directory = t.TempDir()
	n.Target.Window.ProcessID = 7
	n.Target.Window.ProcessStartedAt = 9
	c := duplex.NewCoordinator(n, store)
	if _, e = n.repair(ctx, c); e == nil || !strings.Contains(e.Error(), "writer_profile_unverified") {
		t.Fatal("repair deadlocked or ignored private proof", e)
	}
	release, e := n.WritersDrained(ctx)
	if e != nil {
		t.Fatal("repair retained writer leases", e)
	}
	release()
}

func TestOldCancellationDoesNotBlockNewRequest(t *testing.T) {
	st := duplex.State{Active: &duplex.ActiveRequest{RequestID: "current", Digest: "digest-current"}, Intents: map[string]duplex.Intent{"cancel": {Message: duplex.Message{Header: duplex.Header{RequestID: "previous", RequestSHA256: "digest-previous"}}}}}
	if e := businessControlGuard(st); e != nil {
		t.Fatal("old cancel blocked next request", e)
	}
	in := st.Intents["cancel"]
	in.Message.Header.RequestID = "current"
	in.Message.Header.RequestSHA256 = "digest-current"
	st.Intents["cancel"] = in
	if e := businessControlGuard(st); e == nil {
		t.Fatal("current cancel did not stop new commit")
	}
}

func TestRequestKeyMatchesFullLogicalIdentity(t *testing.T) {
	source := []byte("return 42")
	frames, e := duplex.NewFrames(hostIdentity, "66666666666666666666666666666666", math.MaxUint64, 1, 1234, 1000, source)
	if e != nil {
		t.Fatal(e)
	}
	st := duplex.State{Identity: hostIdentity, Active: &duplex.ActiveRequest{RequestID: frames[0].Header.RequestID, Digest: frames[0].Header.RequestSHA256, Sequence: math.MaxUint64, Created: 1234, Budget: 1000}}
	record := requestRecord{Key: "opaque", SourceSHA256: digestBytes(source), Budget: 1, Sequence: math.MaxUint64, RequestID: frames[0].Header.RequestID}
	if e = matchRequestRecord(record, st, source); e != nil {
		t.Fatal(e)
	}
	st.Active.Digest = digestBytes([]byte("wrong"))
	if e = matchRequestRecord(record, st, source); e == nil {
		t.Fatal("accepted same sequence with different digest")
	}
}

func TestBoundedResultFilesAndPersistenceSentinel(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "result.bin")
	f, e := os.Create(path)
	if e != nil {
		t.Fatal(e)
	}
	if e = f.Truncate(duplex.MaxResultBytes + 1); e != nil {
		t.Fatal(e)
	}
	f.Close()
	if _, e = boundedBytes(path, duplex.MaxResultBytes); e == nil {
		t.Fatal("read oversized result")
	}
	if e = os.Mkdir(filepath.Join(dir, "directory"), 0700); e != nil {
		t.Fatal(e)
	}
	if e = writeJSON(context.Background(), filepath.Join(dir, "directory"), map[string]string{}); !errors.Is(e, duplex.ErrPersistence) {
		t.Fatal("persistence error lost sentinel", e)
	}
}

func TestInspectValidatesJournalWithoutCreatingLease(t *testing.T) {
	p := &Project{Root: t.TempDir()}
	id := "CON-11111111111111111111111111111111"
	dir := p.path(id)
	if e := os.MkdirAll(dir, 0700); e != nil {
		t.Fatal(e)
	}
	meta := targetRecord{Schema: "lycheedev.duplex.target.v1", Claim: journal.WindowOwner{OperationID: id, WorkspaceID: p.workspaceID()}}
	write := func(name string, value any) {
		t.Helper()
		body, e := json.Marshal(value)
		if e != nil {
			t.Fatal(e)
		}
		if e = os.WriteFile(filepath.Join(dir, name), body, 0600); e != nil {
			t.Fatal(e)
		}
	}
	write("target.json", meta)
	for _, invalid := range []duplex.State{
		{Schema: "lycheedev.duplex.journal.v1", Bound: true, Intents: map[string]duplex.Intent{}},
		{Schema: "lycheedev.duplex.journal.v1", Identity: hostIdentity, Intents: map[string]duplex.Intent{"cancel": {Outcome: duplex.WriteOutcome{State: "invented"}}}},
		{Schema: "lycheedev.duplex.journal.v1", Identity: hostIdentity, Bound: true, Intents: map[string]duplex.Intent{}, Active: &duplex.ActiveRequest{RequestID: "bad", Digest: "bad"}},
	} {
		write("state.json", invalid)
		before, e := os.ReadDir(dir)
		if e != nil {
			t.Fatal(e)
		}
		if _, e = p.Inspect(context.Background(), id); e == nil || !strings.Contains(e.Error(), "duplex_journal_invalid") {
			t.Fatal("read-only inspection accepted invalid durable state", e)
		}
		after, e := os.ReadDir(dir)
		if e != nil || len(after) != len(before) {
			t.Fatal("inspection created journal artifacts", e)
		}
		if _, e = os.Stat(filepath.Join(dir, "locks")); !os.IsNotExist(e) {
			t.Fatal("inspection created a lease", e)
		}
	}
}
