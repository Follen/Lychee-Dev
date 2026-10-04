//go:build windows && amd64

package duplexhost

import (
	"context"
	"encoding/hex"
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
	busy, e := vault.AcquireLease(ctx, n.scope(), n.resource("stop"))
	if e != nil {
		t.Fatal(e)
	}
	defer busy.Close()
	closeLane, e := vault.TryAcquireLease(ctx, n.scope(), n.resource("command"))
	if e != nil {
		t.Fatal("stop blocked independent command", e)
	}
	closeLane.Close()
	if release, e := n.WritersDrained(ctx); e == nil {
		release()
		t.Fatal("drain accepted active stop writer")
	}
	data, e := vault.TryAcquireLease(ctx, n.scope(), n.resource("command"))
	if e != nil {
		t.Fatal("failed drain retained earlier command lock", e)
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
	box := duplex.Sendbox{Identity: hostIdentity, Schema: "lycheedev.mailbox.v1", LayoutID: duplex.MailboxLayoutID, Phase: "idle", TransportReady: true, ControlReady: true, Heartbeat: 1, StatusSequence: 1}
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
	st := duplex.State{Active: &duplex.ActiveRequest{RequestID: "current", Digest: "digest-current"}, Intents: map[string]duplex.Intent{"stop": {Message: duplex.Message{Header: duplex.Header{RequestID: "previous", RequestSHA256: "digest-previous", Kind: duplex.Cancel}}}}}
	in := st.Intents["stop"]
	in.Accepted = true
	st.Intents["stop"] = in
	if e := businessControlGuard(st); e != nil {
		t.Fatal("accepted old stop blocked next request", e)
	}
	in = st.Intents["stop"]
	in.Accepted = false
	in.Message.Header.RequestID = "current"
	in.Message.Header.RequestSHA256 = "digest-current"
	st.Intents["stop"] = in
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
		{Schema: "lycheedev.duplex.journal.v1", Identity: hostIdentity, Intents: map[string]duplex.Intent{"stop": {Outcome: duplex.WriteOutcome{State: "invented"}}}},
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

func TestHistoricalResultReleaseRequiresAcceptedJointACK(t *testing.T) {
	record := requestRecord{RequestID: "previous", Sequence: 1}
	st := duplex.State{Active: &duplex.ActiveRequest{RequestID: "next", Sequence: 2, PreviousResultAck: &duplex.ResultManifest{RequestID: "previous"}}}
	if historicalResultReleased(st, record) {
		t.Fatal("staged next request treated as previous ACK")
	}
	st.Active.NextFrame = 1
	if !historicalResultReleased(st, record) {
		t.Fatal("accepted exact previous ACK not projected")
	}
	st.Active.Sequence = 3
	st.Active.NextFrame = 0
	if !historicalResultReleased(st, record) {
		t.Fatal("later accepted sequence lost historical release")
	}
}
func TestSameBuildProcessRowLocksUseCreationIdentity(t *testing.T) {
	first, second := &Native{}, &Native{}
	first.Target.Window.ProcessID = 7
	first.Target.Window.ProcessStartedAt = 9
	second.Target.Window.ProcessID = 8
	second.Target.Window.ProcessStartedAt = 9
	if first.resource("command") == second.resource("command") {
		t.Fatal("same-build instances share row lease")
	}
	second.Target.Window.ProcessID = 7
	second.Target.Window.ProcessStartedAt = 10
	if first.resource("stop") == second.resource("stop") {
		t.Fatal("reused PID inherits stale lease")
	}
}

func TestProjectInspectAcceptsRealCompactedActiveJournal(t *testing.T) {
	ctx := context.Background()
	p := &Project{Root: t.TempDir()}
	id := "CON-11111111111111111111111111111111"
	dir := p.path(id)
	store := duplex.NewFileStore(dir)
	source := []byte("return 42 -- persisted active command")
	created := uint64(time.Now().UnixMilli())
	frames, e := duplex.NewFrames(hostIdentity, strings.Repeat("6", 32), 1, 1, created, 1000, source)
	if e != nil {
		t.Fatal(e)
	}
	result := []byte(`{"value":42}`)
	manifest, _ := resultFixture(result)
	manifest.RequestSHA256 = frames[0].Header.RequestSHA256
	if e = store.SaveResult(ctx, manifest, result); e != nil {
		t.Fatal(e)
	}
	if e = store.Update(ctx, func(st *duplex.State) error {
		st.Identity = hostIdentity
		st.Selected = true
		st.Bound = true
		st.RequestSequence = 1
		st.Active = &duplex.ActiveRequest{RequestID: manifest.RequestID, Digest: manifest.RequestSHA256, Sequence: 1, Attempt: 1, Created: created, Budget: 1000, TransferDeadline: created + 600000, Source: source, Frames: frames, NextFrame: 1, Result: &manifest, ResultSaved: true, Phase: "result_pending"}
		st.Intents["command"] = duplex.Intent{Message: frames[0], Outcome: duplex.WriteOutcome{State: duplex.CompleteWrite}, Accepted: true}
		return nil
	}); e != nil {
		t.Fatal(e)
	}
	meta := targetRecord{Schema: "lycheedev.duplex.target.v1", Identity: hostIdentity, Claim: journal.WindowOwner{OperationID: id, WorkspaceID: p.workspaceID()}}
	if e = writeJSON(ctx, filepath.Join(dir, "target.json"), meta); e != nil {
		t.Fatal(e)
	}
	path := filepath.Join(dir, "state.json")
	before, e := os.ReadFile(path)
	if e != nil {
		t.Fatal(e)
	}
	var compact duplex.State
	if e = json.Unmarshal(before, &compact); e != nil || compact.Active.Frames[0].Payload != nil || compact.Intents["command"].Message.Payload != nil {
		t.Fatal("fixture is not a compacted actual FileStore journal", e)
	}
	lease, e := vault.AcquireLease(ctx, filepath.Join(dir, "locks"), "duplex-journal")
	if e != nil {
		t.Fatal(e)
	}
	defer lease.Close()
	limited, cancel := context.WithTimeout(ctx, time.Second)
	defer cancel()
	inspected, inspectErr := p.Inspect(limited, id)
	// The owned fixture has no game target. Reaching process diagnosis and
	// preserving the verified report prove the journal inspection completed.
	if inspectErr == nil || strings.Contains(inspectErr.Error(), "duplex_journal_invalid") || inspected.Diagnostics["processIdentity"].State != "unavailable" || inspected.ReportState != "verified" || inspected.Operation != manifest.RequestID {
		t.Fatal("project doctor rejected compressed active state before process inspection", inspectErr, inspected)
	}
	after, e := os.ReadFile(path)
	if e != nil || string(before) != string(after) {
		t.Fatal("project inspection changed journal", e)
	}
}

func retirementFixture(t *testing.T, pending bool) (*Project, string, targetRecord, *duplex.FileStore, duplex.State) {
	t.Helper()
	ctx := context.Background()
	p := &Project{Root: t.TempDir()}
	id := "CON-" + strings.Repeat("1", 32)
	store := duplex.NewFileStore(p.path(id))
	meta := targetRecord{Schema: "lycheedev.duplex.target.v1", Identity: hostIdentity, ActorGUID: "actor-old"}
	meta.Target.Window.ProcessID = 7
	meta.Target.Window.ProcessStartedAt = 9
	meta.Target.Window.Handle = 10
	meta.Target.Window.Executable = filepath.Join(p.Root, "Wow.exe")
	meta.Target.Client.Directory = filepath.Join(p.Root, "client")
	if e := os.MkdirAll(addonParent(meta.Target), 0700); e != nil {
		t.Fatal(e)
	}
	meta.Claim = journal.WindowOwner{Schema: "lycheedev.window-owner.v1", WorkspaceID: p.workspaceID(), Resource: "window/7/9/10", OperationID: id, IntentSHA256: strings.Repeat("a", 64)}
	if e := journal.BeginConnectionWindow(ctx, addonParent(meta.Target), meta.Claim, func() error { return writeJSON(ctx, filepath.Join(p.path(id), "target.json"), meta) }); e != nil {
		t.Fatal(e)
	}
	if e := store.Update(ctx, func(st *duplex.State) error {
		st.Identity = hostIdentity
		st.Selected = true
		st.Bound = true
		if pending {
			source := []byte("return42")
			created := uint64(time.Now().UnixMilli())
			frames, e := duplex.NewFrames(hostIdentity, strings.Repeat("6", 32), 1, 1, created, 1000, source)
			if e != nil {
				return e
			}
			st.RequestSequence = 1
			st.PublicationSequence = 1
			st.Active = &duplex.ActiveRequest{RequestID: frames[0].Header.RequestID, Digest: frames[0].Header.RequestSHA256, Sequence: 1, Attempt: 1, Created: created, Budget: 1000, TransferDeadline: created + 600000, Source: source, Frames: frames, Phase: "submitted"}
			st.Intents["command"] = duplex.Intent{Message: frames[0], Outcome: duplex.WriteOutcome{State: duplex.UnknownWrite}}
		}
		return nil
	}); e != nil {
		t.Fatal(e)
	}
	st, e := store.Load(ctx)
	if e != nil {
		t.Fatal(e)
	}
	return p, id, meta, store, st
}
func TestManualLifecycleChangeRetiresOnlyLocalClaimAndKeepsUnknown(t *testing.T) {
	for _, kind := range []string{"idle_reload", "actor_switch", "pending_unknown"} {
		t.Run(kind, func(t *testing.T) {
			p, id, meta, store, st := retirementFixture(t, kind == "pending_unknown")
			observed := duplex.Sendbox{Identity: hostIdentity, ActorReady: true, ActorGUID: meta.ActorGUID}
			if kind == "actor_switch" {
				observed.ActorBinding = strings.Repeat("c", 32)
				observed.ActorGUID = "actor-new"
			} else {
				observed.Runtime = strings.Repeat("b", 32)
			}
			doctor := ProjectResult{Target: &meta.Target, Status: &observed, Diagnostics: map[string]Diagnostic{"processIdentity": {"verified", ""}, "runtimeFresh": {"advancing", ""}}}
			result, e := p.retireChangedRuntime(context.Background(), id, meta, store, st, doctor)
			if e != nil || result.Cleanup != "local_retired" || result.Closed {
				t.Fatal("lifecycle change claimed addon close or failed local retirement", e, result)
			}
			latest, e := store.Load(context.Background())
			if e != nil || !latest.LocalRetired || latest.LocalRetirement == nil || latest.Identity != hostIdentity {
				t.Fatal(e)
			}
			if kind == "pending_unknown" && (latest.Active == nil || latest.Active.RequestID != st.Active.RequestID || latest.Active.ResultSaved || latest.Active.Released || latest.Intents["command"].Outcome.State != duplex.UnknownWrite || result.Complete || result.Stage != "execution_unknown") {
				t.Fatal("manual reload erased original unknown effect")
			}
			_, busy, e := journal.InspectWindowOwner(context.Background(), addonParent(meta.Target), meta.Claim.Resource)
			if e != nil || busy {
				t.Fatal("old driver claim retained", e)
			}
			nextClaim := meta.Claim
			nextClaim.OperationID = "CON-" + strings.Repeat("2", 32)
			if e = journal.BeginConnectionWindow(context.Background(), addonParent(meta.Target), nextClaim, func() error { return nil }); e != nil {
				t.Fatal("replacement instance cannot acquire fresh local selection", e)
			}
		})
	}
}
func TestLifecycleRetirementRequiresExactFreshDoctorAndWriterDrain(t *testing.T) {
	p, id, meta, store, st := retirementFixture(t, true)
	observed := duplex.Sendbox{Identity: hostIdentity}
	observed.Runtime = strings.Repeat("b", 32)
	doctor := ProjectResult{Target: &meta.Target, Status: &observed, Diagnostics: map[string]Diagnostic{"processIdentity": {"verified", ""}, "runtimeFresh": {"unknown", ""}}}
	if _, e := p.retireChangedRuntime(context.Background(), id, meta, store, st, doctor); !errors.Is(e, duplex.ErrUnknown) {
		t.Fatal("stale heartbeat retired unknown effect", e)
	}
	doctor.Diagnostics["runtimeFresh"] = Diagnostic{"advancing", ""}
	native := &Native{Target: meta.Target}
	held, e := vault.TryAcquireLease(context.Background(), native.scope(), native.resource("command"))
	if e != nil {
		t.Fatal(e)
	}
	if _, e = p.retireChangedRuntime(context.Background(), id, meta, store, st, doctor); e == nil {
		t.Fatal("retired while old row writer active")
	}
	_, busy, e := journal.InspectWindowOwner(context.Background(), addonParent(meta.Target), meta.Claim.Resource)
	if e != nil || !busy {
		t.Fatal("writer drain failure removed claim", e)
	}
	held.Close()
	latest, e := store.Load(context.Background())
	if e != nil {
		t.Fatal(e)
	}
	if _, e = p.retireLocalSelection(context.Background(), id, meta, latest); e != nil {
		t.Fatal("local retirement could not recover after exact writer drained", e)
	}
}
func TestReloadRuntimeReplacementSucceedsWithoutObservedLeaseReceipt(t *testing.T) {
	p, id, meta, store, st := retirementFixture(t, false)
	frames, e := duplex.NewFrames(hostIdentity, strings.Repeat("6", 32), 1, 1, uint64(time.Now().UnixMilli()), 1000, []byte("return42"))
	if e != nil {
		t.Fatal(e)
	}
	header := frames[0].Header
	header.Kind = duplex.Reload
	header.RequestID = strings.Repeat("0", 32)
	header.RequestSHA256 = strings.Repeat("0", 64)
	header.RequestSeq = 0
	header.BudgetMillis = 0
	header.TotalBytes = 0
	header.PayloadBytes = 0
	header.FrameIndex = 0
	header.FrameCount = 0
	prepared := duplex.Intent{Message: duplex.Message{Header: header}, Accepted: true, Challenge: strings.Repeat("d", 32), Outcome: duplex.WriteOutcome{State: duplex.CompleteWrite}}
	payload, e := hex.DecodeString(header.MessageID)
	if e != nil {
		t.Fatal(e)
	}
	header.Kind = duplex.Lease
	header.Challenge = prepared.Challenge
	header.PublicationSeq = 2
	header.PublicationBegin = 4
	header.PublicationEnd = 4
	header.MessageID = strings.Repeat("e", 32)
	header.PayloadBytes = 16
	if e = store.Update(context.Background(), func(current *duplex.State) error {
		current.ReloadPrepared = &prepared
		current.PublicationSequence = 2
		current.Intents["stop"] = duplex.Intent{Message: duplex.Message{Header: header, Payload: payload}, Outcome: duplex.WriteOutcome{State: duplex.UnknownWrite}}
		return nil
	}); e != nil {
		t.Fatal(e)
	}
	replacement := duplex.Sendbox{Identity: hostIdentity}
	replacement.Runtime = strings.Repeat("b", 32)
	replacement.ResourcesReleased = true
	native := &Native{Target: meta.Target, observe: func(context.Context) (duplex.Sendbox, error) { return replacement, nil }}
	coordinator := duplex.NewCoordinator(native, store)
	retired, e := p.resumeReload(context.Background(), id, coordinator)
	if e != nil || !retired.Closed || retired.Identity != st.Identity {
		t.Fatal("expected old-runtime identity loss surfaced as completed reload error", e)
	}
	saved, e := p.metadata(id)
	if e != nil || saved.RuntimeRetirement == nil || saved.RuntimeRetirement.ToRuntime != replacement.Runtime {
		t.Fatal("replacement evidence not durable", e)
	}
}
func TestValidationDoctorFreshnessKeepsRetainedIdentity(t *testing.T) {
	previous := duplex.Sendbox{Identity: hostIdentity, Schema: "lycheedev.mailbox.v1", LayoutID: duplex.MailboxLayoutID, Phase: "validating", ClosedAdmission: true, ResourcesReleased: true, ActorReady: true, ControlReady: true, TransportReady: true, ReadyChallenge: strings.Repeat("a", 32), AdmissionSequence: 1, StatusSequence: 1, Heartbeat: 1, Validation: &duplex.ValidationProgress{RequestID: strings.Repeat("6", 32), RequestSHA256: strings.Repeat("0", 64), TotalBytes: 100, CopiedBytes: 20, HashedBytes: 10}}
	wire, e := duplex.EncodeSendbox(previous)
	if e != nil {
		t.Fatal(e)
	}
	decoded, e := duplex.DecodeSendbox(wire)
	if e != nil || decoded.Validation.HashedBytes != 10 {
		t.Fatal("new validation sendbox was not decodable", e)
	}
	next := decoded
	next.StatusSequence++
	next.Heartbeat++
	next.Validation = &duplex.ValidationProgress{RequestID: decoded.Validation.RequestID, RequestSHA256: decoded.Validation.RequestSHA256, TotalBytes: 100, CopiedBytes: 30, HashedBytes: 20}
	if advancing, e := statusAdvance(decoded, next); e != nil || !advancing {
		t.Fatal("doctor could not verify advancing validation heartbeat", e)
	}
	next.Owner = strings.Repeat("b", 32)
	if _, e = statusAdvance(decoded, next); !errors.Is(e, duplex.ErrIdentity) {
		t.Fatal("identity consumption mixed old/new doctor samples", e)
	}
}

func TestChangedRuntimeRetirementWaitsForExactDriverDrain(t *testing.T) {
	p, id, meta, store, st := retirementFixture(t, true)
	observed := duplex.Sendbox{Identity: hostIdentity}
	observed.Runtime = strings.Repeat("b", 32)
	doctor := ProjectResult{Target: &meta.Target, Status: &observed, Diagnostics: map[string]Diagnostic{"processIdentity": {"verified", ""}, "runtimeFresh": {"advancing", ""}}}
	driver, e := journal.LockBootstrapWindow(context.Background(), addonParent(meta.Target), meta.Claim)
	if e != nil {
		t.Fatal(e)
	}
	if _, e = p.retireChangedRuntime(context.Background(), id, meta, store, st, doctor); e == nil {
		t.Fatal("retired an active exact process driver")
	}
	_, busy, e := journal.InspectWindowOwner(context.Background(), addonParent(meta.Target), meta.Claim.Resource)
	if e != nil || !busy {
		t.Fatal("driver drain failure removed claim", e)
	}
	driver.Close()
	latest, e := store.Load(context.Background())
	if e != nil {
		t.Fatal(e)
	}
	if _, e = p.retireLocalSelection(context.Background(), id, meta, latest); e != nil {
		t.Fatal("local retirement failed after driver release", e)
	}
}

func TestProjectIdleDisconnectNeedsNoRemoteDoctorOrRuntime(t *testing.T) {
	ctx := context.Background()
	p, id, meta, store, _ := retirementFixture(t, false)
	if e := store.Update(ctx, func(st *duplex.State) error { st.Bound = false; return nil }); e != nil {
		t.Fatal(e)
	}
	// The target fixture has no live window, executable, addon, or heartbeat.
	// The public entry must still release a never-published local selection.
	result, e := p.Disconnect(ctx, id, false)
	if e != nil || result.Bound || result.Closed || result.Cleanup != "complete" || result.Stage != "retired" || result.Diagnostics != nil || result.Status != nil {
		t.Fatal("idle disconnect attempted remote diagnosis or claimed addon close", e, result)
	}
	saved, e := store.Load(ctx)
	if e != nil || !saved.LocalRetired || !saved.Closing || saved.Bound || saved.Closed || len(saved.Intents) != 0 || saved.PublicationSequence != 0 {
		t.Fatal("local-only retirement was not durable", e)
	}
	_, busy, e := journal.InspectWindowOwner(ctx, addonParent(meta.Target), meta.Claim.Resource)
	if e != nil || busy {
		t.Fatal("idle claim remains occupied", e)
	}
	if result, e = p.Disconnect(ctx, id, false); e != nil || result.Cleanup != "complete" {
		t.Fatal("repeated local disconnect was not read-only complete", e)
	}
}
func TestProjectIdleDisconnectWaitsForLocalDriverWithoutRemoteDoctor(t *testing.T) {
	ctx := context.Background()
	p, id, meta, store, _ := retirementFixture(t, false)
	if e := store.Update(ctx, func(st *duplex.State) error { st.Bound = false; return nil }); e != nil {
		t.Fatal(e)
	}
	driver, e := journal.LockBootstrapWindow(ctx, addonParent(meta.Target), meta.Claim)
	if e != nil {
		t.Fatal(e)
	}
	result, e := p.Disconnect(ctx, id, false)
	if e == nil || result.Diagnostics != nil || result.Status != nil {
		t.Fatal("active local driver did not block retirement locally", e)
	}
	_, busy, e := journal.InspectWindowOwner(ctx, addonParent(meta.Target), meta.Claim.Resource)
	if e != nil || !busy {
		t.Fatal("active driver claim was removed", e)
	}
	driver.Close()
	if result, e = p.Disconnect(ctx, id, false); e != nil || result.Cleanup != "complete" {
		t.Fatal("retry after local driver release depended on runtime", e)
	}
}
func TestProjectDisconnectDoesNotTreatUnknownCommandAsUnusedSelection(t *testing.T) {
	ctx := context.Background()
	p, id, meta, store, _ := retirementFixture(t, true)
	if e := store.Update(ctx, func(st *duplex.State) error { st.Bound = false; return nil }); e != nil {
		t.Fatal(e)
	}
	if _, e := p.Disconnect(ctx, id, false); e == nil {
		t.Fatal("unknown publication was silently locally retired without lifecycle proof")
	}
	saved, e := store.Load(ctx)
	if e != nil || saved.LocalRetired || saved.Active == nil || saved.Intents["command"].Outcome.State != duplex.UnknownWrite {
		t.Fatal("unknown request evidence lost", e)
	}
	_, busy, e := journal.InspectWindowOwner(ctx, addonParent(meta.Target), meta.Claim.Resource)
	if e != nil || !busy {
		t.Fatal("unknown effect claim was cleared", e)
	}
}
