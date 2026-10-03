package duplex

import (
	"context"
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"strings"
	"sync"
	"testing"
	"time"
)

var fixtureIdentity = Identity{Runtime: "11111111111111111111111111111111", Arena: "22222222222222222222222222222222", Session: "33333333333333333333333333333333", Owner: "44444444444444444444444444444444", ActorBinding: "55555555555555555555555555555555", Fence: 1}

func TestWireBoundariesAndTampering(t *testing.T) {
	for _, n := range []int{0, 1, 4095, 4096, 4097, MaxSourceBytes - 1, MaxSourceBytes} {
		t.Run(fmt.Sprint(n), func(t *testing.T) {
			source := make([]byte, n)
			for i := range source {
				source[i] = byte(i)
			}
			frames, e := NewFrames(fixtureIdentity, "66666666666666666666666666666666", 1, 1, 1234, 1000, source)
			if e != nil {
				t.Fatal(e)
			}
			var out []byte
			for _, m := range frames {
				wire, e := EncodeMessage(m)
				if e != nil {
					t.Fatal(e)
				}
				decoded, e := DecodeMessage(wire)
				if e != nil {
					t.Fatal(e)
				}
				out = append(out, decoded.Payload...)
				for _, offset := range []int{12, 60, 148, 180, 200, 232, 264, 296, 304, 312} {
					bad := append([]byte(nil), wire...)
					bad[offset] ^= 1
					if _, e = DecodeMessage(bad); e == nil {
						t.Fatalf("accepted damaged offset %d", offset)
					}
				}
			}
			d, e := RequestDigest(frames[0].Header, out)
			if e != nil || d != frames[0].Header.RequestSHA256 {
				t.Fatal("reassembly digest mismatch", e)
			}
		})
	}
	if _, e := NewFrames(fixtureIdentity, "66666666666666666666666666666666", 1, 1, 1, 1, make([]byte, MaxSourceBytes+1)); e == nil {
		t.Fatal("accepted oversized source")
	}
}

func TestLogicalDigestExcludesRepairIdentity(t *testing.T) {
	frames, e := NewFrames(fixtureIdentity, "66666666666666666666666666666666", 9007199254740993, 1, 1234, 1000, []byte("return 42\r\n"))
	if e != nil {
		t.Fatal(e)
	}
	h := frames[0].Header
	before := h.RequestSHA256
	h.Arena = "77777777777777777777777777777777"
	h.Session = "88888888888888888888888888888888"
	h.TransportAttempt = 2
	after, e := RequestDigest(h, frames[0].Payload)
	if e != nil || before != after {
		t.Fatal("repair changed logical digest")
	}
	h.BudgetMillis++
	changed, _ := RequestDigest(h, frames[0].Payload)
	if changed == before {
		t.Fatal("budget not included")
	}
	h.BudgetMillis--
	h.ActorBinding = "99999999999999999999999999999999"
	changed, _ = RequestDigest(h, frames[0].Payload)
	if changed == before {
		t.Fatal("actor not included")
	}
	wire, e := EncodeMessage(frames[0])
	if e != nil {
		t.Fatal(e)
	}
	if binary.LittleEndian.Uint64(wire[156:164]) != 9007199254740993 {
		t.Fatal("u64 rounded")
	}
	// Fixed cross-language request/header vector (exact CRLF is significant).
	if before != "35680b5b9ee65ad4eb056008c462d591aaae2327d17d52c377c4c016c9238881" {
		t.Fatal("request golden mismatch", before)
	}
	frames[0].Header.MessageID = "77777777777777777777777777777777"
	wire, e = EncodeMessage(frames[0])
	if e != nil {
		t.Fatal(e)
	}
	if hex.EncodeToString(wire[232:264]) != "fb7da94043e1d06f3867c15f93a52688345e33adc79f64c42568955e7285f345" || hex.EncodeToString(wire[264:296]) != "4fe91111ad00870d3f5f2b4ff9d481d8fe9b516ffee3817e9a904370803b2542" {
		t.Fatal("cross language frame/header golden mismatch")
	}
}

func TestSendboxDigestAndU64(t *testing.T) {
	s := fixtureSendbox()
	s.StatusSequence = 9007199254740993
	b, e := EncodeSendbox(s)
	if e != nil {
		t.Fatal(e)
	}
	out, e := DecodeSendbox(b)
	if e != nil || out.StatusSequence != s.StatusSequence {
		t.Fatal("uint64 roundtrip", e)
	}
	b[len(b)-1] ^= 1
	if _, e = DecodeSendbox(b); e == nil {
		t.Fatal("accepted corrupted sendbox")
	}
	m := ResultManifest{RequestID: "66666666666666666666666666666666", RequestSHA256: "0000000000000000000000000000000000000000000000000000000000000000", State: "success", SHA256: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855", Bytes: 0, Pages: 1, PageSHA256: []string{"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"}}
	if e = ValidateResult(m, nil); e != nil {
		t.Fatal(e)
	}
	if e = ValidateResult(m, []byte{0}); e == nil {
		t.Fatal("accepted extra result byte")
	}
}

type fixtureStore struct {
	mu         sync.Mutex
	state      State
	results    map[string][]byte
	failUpdate bool
	failResult bool
}

func newFixtureStore() *fixtureStore {
	return &fixtureStore{state: State{Schema: "lycheedev.duplex.journal.v1", Intents: map[string]Intent{}}, results: map[string][]byte{}}
}
func copyState(s State) State {
	p, _ := json.Marshal(s)
	var out State
	_ = json.Unmarshal(p, &out)
	return out
}
func (s *fixtureStore) Load(context.Context) (State, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	return copyState(s.state), nil
}
func (s *fixtureStore) Update(_ context.Context, f func(*State) error) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.failUpdate {
		s.failUpdate = false
		return errors.New("sync failed")
	}
	st := copyState(s.state)
	if e := f(&st); e != nil {
		return e
	}
	s.state = st
	return nil
}
func (s *fixtureStore) SaveResult(_ context.Context, m ResultManifest, p []byte) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.failResult {
		return errors.New("result sync failed")
	}
	if e := ValidateResult(m, p); e != nil {
		return e
	}
	s.results[m.RequestID] = append([]byte(nil), p...)
	return nil
}

func fixtureSendbox() Sendbox {
	return Sendbox{Identity: fixtureIdentity, Schema: "lycheedev.duplex.v1", Phase: "idle", Ready: true, ActorReady: true, TransportReady: true, BusinessReady: true, ControlReady: true, StatusSequence: 1, Heartbeat: 1, Receipts: map[string]Receipt{}}
}

type fixtureBackend struct {
	mu            sync.Mutex
	box           Sendbox
	store         *fixtureStore
	writes        []Message
	source        []byte
	executions    int
	unknown       Kind
	stalled       bool
	running       bool
	corruptResult bool
	result        []byte
	reloads       int
	repairNoWrite bool
	bindNoWrite   bool
	reject        Kind
	afterWrite    func()
}

func newFixtureBackend(s *fixtureStore) *fixtureBackend {
	return &fixtureBackend{box: fixtureSendbox(), store: s}
}
func (b *fixtureBackend) Observe(context.Context) (Sendbox, error) {
	b.mu.Lock()
	defer b.mu.Unlock()
	b.box.Heartbeat++
	b.box.Ready = b.box.BusinessReady
	b.box.StatusSequence++
	p, _ := json.Marshal(b.box)
	var out Sendbox
	_ = json.Unmarshal(p, &out)
	return out, nil
}
func (b *fixtureBackend) Publish(ctx context.Context, m Message) (WriteOutcome, error) {
	b.mu.Lock()
	defer b.mu.Unlock()
	st, _ := b.store.Load(ctx)
	in, ok := st.Intents[m.Header.Kind.Lane()]
	if !ok || in.Message.Header.MessageID != m.Header.MessageID || in.Outcome.State != UnknownWrite {
		return WriteOutcome{State: NoWrite}, errors.New("intent not durable before effect")
	}
	wire, e := EncodeMessage(m)
	if e != nil {
		return WriteOutcome{State: NoWrite}, e
	}
	decoded, e := DecodeMessage(wire)
	if e != nil {
		return WriteOutcome{State: NoWrite}, e
	}
	m = decoded
	h := m.Header
	if h.Kind == Bind {
		if e = ValidateBindTransition(b.box, m); e != nil {
			return WriteOutcome{State: NoWrite}, e
		}
		if b.bindNoWrite {
			return WriteOutcome{State: NoWrite}, errors.New("bind proven no write")
		}
	} else if !exact(Identity{h.Runtime, h.Arena, h.Session, h.Owner, h.ActorBinding, h.Fence}, b.box) {
		return WriteOutcome{State: NoWrite}, ErrIdentity
	}
	b.writes = append(b.writes, m)
	if h.Kind == Repair && b.repairNoWrite {
		return WriteOutcome{State: NoWrite}, errors.New("repair proven no write")
	}
	if h.Kind == b.reject {
		b.receipt(m, "challenge_expired")
		return WriteOutcome{State: CompleteWrite, Bytes: uint64(len(wire))}, nil
	}
	if !b.stalled {
		switch h.Kind {
		case Bind:
			if b.box.Identity.Session != h.Session {
				b.box.Phase = "idle"
				b.box.BusinessReady = true
				b.box.Receipts = map[string]Receipt{}
			}
			b.box.Identity = Identity{h.Runtime, h.Arena, h.Session, h.Owner, h.ActorBinding, h.Fence}
			b.receipt(m, "bound")
		case Frame:
			if b.box.Request == nil {
				b.box.Request = &RequestState{RequestID: h.RequestID, RequestSHA256: h.RequestSHA256, RequestSeq: h.RequestSeq, TransportAttempt: h.TransportAttempt, NotStarted: true}
				b.box.BusinessReady = false
				b.source = nil
			}
			if h.FrameIndex != uint32(len(b.box.Request.AcceptedFrames)+1) {
				return WriteOutcome{State: NoWrite}, errors.New("fixture out of order")
			}
			b.source = append(b.source, m.Payload...)
			b.box.Request.AcceptedFrames = append(b.box.Request.AcceptedFrames, h.FrameIndex)
			b.box.Phase = "receiving"
			if h.FrameIndex == h.FrameCount {
				d, err := RequestDigest(h, b.source)
				if err != nil || d != h.RequestSHA256 {
					return WriteOutcome{State: UnknownWrite}, errors.New("logical digest rejected")
				}
				b.box.Phase = "prepared"
				b.box.Request.Challenge = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
			}
		case Commit:
			if b.box.Phase != "prepared" || h.Challenge != b.box.Request.Challenge || h.RequestSHA256 != b.box.Request.RequestSHA256 {
				return WriteOutcome{State: NoWrite}, errors.New("commit challenge rejected")
			}
			b.executions++
			b.box.Request.NotStarted = false
			b.box.Phase = "running"
			b.receipt(m, "accepted")
			if !b.running {
				b.terminal(h, "success", []byte(`{"value":42}`))
			}
		case Cancel:
			if b.box.Terminal != nil {
				b.receipt(m, "cancel_too_late")
			} else {
				b.receipt(m, "accepted")
				b.terminal(h, "cancelled", []byte(`{"cancelled":true}`))
			}
		case Close:
			b.receipt(m, "closed")
			b.box.Phase = "closed"
			b.box.BusinessReady = false
			if b.box.Request == nil && b.box.Terminal == nil {
				b.box.ResourcesReleased = true
			}
		case ResultAck:
			if b.store.results[h.RequestID] == nil && b.box.Terminal.Bytes != 0 {
				return WriteOutcome{State: NoWrite}, errors.New("ACK before result durable")
			}
			manifest, err := DecodeResultAck(m.Payload)
			manifest.PageSHA256 = b.box.Terminal.PageSHA256
			expected, _ := EncodeResultAck(*b.box.Terminal)
			actual, _ := EncodeResultAck(manifest)
			if err != nil || string(expected) != string(actual) {
				return WriteOutcome{State: NoWrite}, errors.New("ACK manifest differs")
			}
			b.box.Released = &Released{h.RequestID, h.RequestSHA256}
			b.box.Request = nil
			b.box.Terminal = nil
			b.box.Phase = "released"
			b.box.BusinessReady = true
			b.box.ResourcesReleased = true
			b.receipt(m, "released")
		case Repair:
			b.box.Repair = nil
			b.box.Request = nil
			b.source = nil
			b.box.Phase = "idle"
			b.receipt(m, "repaired")
		case Reload:
			b.receipt(m, "accepted")
			r := b.box.Receipts["reload"]
			r.Challenge = "abababababababababababababababab"
			b.box.Receipts["reload"] = r
		case Lease:
			prepared := st.Intents["reload"]
			if !prepared.Accepted || prepared.Challenge != h.Challenge || hex.EncodeToString(m.Payload) != prepared.Message.Header.MessageID {
				return WriteOutcome{State: NoWrite}, errors.New("reload lease rejected")
			}
			b.receipt(m, "accepted")
			b.reloads++
		}
	}
	if b.afterWrite != nil {
		b.afterWrite()
	}
	if b.unknown == h.Kind {
		return WriteOutcome{State: UnknownWrite, Bytes: uint64(len(wire))}, errors.New("write outcome interrupted")
	}
	return WriteOutcome{State: CompleteWrite, Bytes: uint64(len(wire)), ReadbackVerified: true}, nil
}
func (b *fixtureBackend) receipt(m Message, state string) {
	h := m.Header
	b.box.Receipts[h.Kind.Lane()] = Receipt{MessageID: h.MessageID, RequestID: h.RequestID, RequestSHA256: h.RequestSHA256, State: state}
}
func (b *fixtureBackend) terminal(h Header, state string, p []byte) {
	d := sha256.Sum256(p)
	b.result = p
	b.box.Terminal = &ResultManifest{RequestID: h.RequestID, RequestSHA256: h.RequestSHA256, State: state, SHA256: hex.EncodeToString(d[:]), Bytes: uint32(len(p)), Pages: 1, PageSHA256: []string{hex.EncodeToString(d[:])}}
	b.box.Phase = "result_pending"
}
func (b *fixtureBackend) ReadResult(context.Context, ResultManifest) ([]byte, error) {
	if b.corruptResult {
		return []byte("damaged"), nil
	}
	return append([]byte(nil), b.result...), nil
}
func (b *fixtureBackend) Close(context.Context) error { return nil }
func newFixtureCoordinator(t *testing.T) (*Coordinator, *fixtureBackend, *fixtureStore) {
	t.Helper()
	s := newFixtureStore()
	b := newFixtureBackend(s)
	c := NewCoordinator(b, s)
	c.PollInterval = time.Millisecond
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	if _, e := c.Connect(ctx, fixtureIdentity); e != nil {
		t.Fatal(e)
	}
	return c, b, s
}

func TestExecuteDurableAckReleaseAndReuse(t *testing.T) {
	c, b, s := newFixtureCoordinator(t)
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	for i := 0; i < 3; i++ {
		st, e := c.Execute(ctx, []byte("return 42"), 1000)
		if e != nil {
			t.Fatal(e)
		}
		if !st.Active.ResultSaved || !st.Active.Released || st.Active.Source != nil || st.Active.Frames != nil {
			t.Fatal("request not accurately released")
		}
	}
	if b.executions != 3 || len(s.results) != 3 {
		t.Fatal("unexpected executions or retained history")
	}
}

func TestUnknownCommitNeverReexecutes(t *testing.T) {
	c, b, _ := newFixtureCoordinator(t)
	b.unknown = Commit
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	st, e := c.Execute(ctx, []byte("return 42"), 1000)
	if e == nil {
		t.Fatal("expected interrupted commit")
	}
	requestID := st.Active.RequestID
	if _, e = c.Resume(ctx); e != nil {
		t.Fatal(e)
	}
	st, _ = c.Store.Load(ctx)
	if b.executions != 1 || st.Active.RequestID != requestID || !st.Active.Released {
		t.Fatal("unknown commit replayed")
	}
	commits := 0
	for _, m := range b.writes {
		if m.Header.Kind == Commit {
			commits++
		}
	}
	if commits != 1 {
		t.Fatal("commit count", commits)
	}
}

func TestSyncFailureStopsEffectAndOutcomeLossRecovers(t *testing.T) {
	c, b, s := newFixtureCoordinator(t)
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	s.failUpdate = true
	if _, e := c.Execute(ctx, []byte("return 42"), 1000); e == nil {
		t.Fatal("expected intent sync failure")
	}
	if len(b.writes) != 1 {
		t.Fatal("wrote before durable intent")
	}
	b.afterWrite = func() {
		if b.writes[len(b.writes)-1].Header.Kind == Commit {
			s.failUpdate = true
			b.afterWrite = nil
		}
	}
	if _, e := c.Execute(ctx, []byte("return 42"), 1000); e == nil {
		t.Fatal("expected lost outcome")
	}
	if _, e := c.Resume(ctx); e != nil {
		t.Fatal(e)
	}
	if b.executions != 1 {
		t.Fatal("outcome loss replayed")
	}
}

func TestResultFailureNeverAcknowledgesAndCloseIndependent(t *testing.T) {
	c, b, s := newFixtureCoordinator(t)
	s.failResult = true
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	if _, e := c.Execute(ctx, []byte("return 42"), 1000); e == nil {
		t.Fatal("expected durability failure")
	}
	for _, m := range b.writes {
		if m.Header.Kind == ResultAck {
			t.Fatal("ACK before result persistence")
		}
	}
	if _, e := c.Disconnect(ctx, 100*time.Millisecond); e != nil {
		t.Fatal(e)
	}
	if b.box.Phase != "closed" {
		t.Fatal("close blocked by result failure")
	}
}

func TestOutstandingCancelAndExpiredBusinessDoNotBlockClose(t *testing.T) {
	c, b, _ := newFixtureCoordinator(t)
	b.running = true
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Millisecond)
	st, e := c.Execute(ctx, []byte("return 42"), 1000)
	cancel()
	if !errors.Is(e, ErrPending) || st.Active == nil {
		t.Fatal("expected running pending", e)
	}
	c.Now = func() time.Time { return time.UnixMilli(int64(st.Active.Created) + 2000) }
	ctx2, cancel2 := context.WithTimeout(context.Background(), time.Second)
	defer cancel2()
	if _, e = c.Step(ctx2); !errors.Is(e, ErrPending) {
		t.Fatal("host creation time must not expire addon execution", e)
	}
	b.stalled = true
	if _, e = c.Cancel(ctx2, 5*time.Millisecond); !errors.Is(e, ErrPending) {
		t.Fatal("expected cancel pending", e)
	}
	b.stalled = false
	if _, e = c.Disconnect(ctx2, 50*time.Millisecond); e != nil {
		t.Fatal(e)
	}
	if b.box.Phase != "closed" || b.executions != 1 {
		t.Fatal("cleanup changed execution")
	}
}

func TestGCRepairRequiresPrivateProofAndDrain(t *testing.T) {
	c, b, _ := newFixtureCoordinator(t)
	b.stalled = true
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Millisecond)
	st, e := c.Execute(ctx, make([]byte, 4097), 1000)
	cancel()
	if !errors.Is(e, ErrPending) {
		t.Fatal(e)
	}
	oldArena := b.box.Arena
	b.box.Arena = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
	b.box.Repair = &RepairProof{PreviousArena: oldArena, NewArena: b.box.Arena, Challenge: "cccccccccccccccccccccccccccccccc", RequestID: st.Active.RequestID, RequestSHA256: st.Active.Digest, NotStarted: true, LedgerRetained: true, PreviousWriterDrained: true}
	ctx2, cancel2 := context.WithTimeout(context.Background(), time.Second)
	defer cancel2()
	before := len(b.writes)
	if _, e = c.Repair(ctx2, false); !errors.Is(e, ErrUnknown) || len(b.writes) != before {
		t.Fatal("repair without drain wrote")
	}
	b.stalled = false
	if _, e = c.Repair(ctx2, true); e != nil {
		t.Fatal(e)
	}
	final, e := c.Resume(ctx2)
	if e != nil {
		t.Fatal(e)
	}
	if final.Active.Attempt != 2 || final.Active.Digest != st.Active.Digest || final.Active.Created != st.Active.Created || final.Active.TransferDeadline != st.Active.TransferDeadline || b.executions != 1 {
		t.Fatal("repair changed logical request")
	}
}

func TestFileStoreDurableRestartAndCorruptEvidence(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	dir := t.TempDir()
	s := NewFileStore(dir)
	if e := s.Update(ctx, func(st *State) error { st.Identity = fixtureIdentity; return nil }); e != nil {
		t.Fatal(e)
	}
	st, e := NewFileStore(dir).Load(ctx)
	if e != nil || st.Identity != fixtureIdentity {
		t.Fatal("restart lost state", e)
	}
	if e = os.WriteFile(dir+"/state.json", []byte(`{"schema":"old"}`), 0600); e != nil {
		t.Fatal(e)
	}
	if _, e = s.Load(ctx); e == nil {
		t.Fatal("accepted corrupt journal")
	}
}

func TestFileStoreMaxSourceAndConcurrentFieldMerges(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	dir := t.TempDir()
	s := NewFileStore(dir)
	source := make([]byte, MaxSourceBytes)
	frames, e := NewFrames(fixtureIdentity, "66666666666666666666666666666666", 1, 1, 1, 120000, source)
	if e != nil {
		t.Fatal(e)
	}
	if e = s.Update(ctx, func(st *State) error {
		st.Identity = fixtureIdentity
		st.Bound = true
		st.RequestSequence = 1
		st.Active = &ActiveRequest{RequestID: frames[0].Header.RequestID, Digest: frames[0].Header.RequestSHA256, Sequence: 1, Attempt: 1, Created: 1, Budget: 120000, TransferDeadline: 600001, Source: source, Frames: frames}
		return nil
	}); e != nil {
		t.Fatal("maximum source journal rejected", e)
	}
	st, e := s.Load(ctx)
	if e != nil || len(st.Active.Source) != MaxSourceBytes || len(st.Active.Frames) != 256 {
		t.Fatal("maximum journal did not roundtrip", e)
	}
	if e = s.Update(ctx, func(st *State) error { st.Active = nil; return nil }); e != nil {
		t.Fatal(e)
	}
	errCh := make(chan error, 2)
	for i := 0; i < 2; i++ {
		go func(field int) {
			other := NewFileStore(dir)
			for n := 0; n < 20; n++ {
				err := other.Update(ctx, func(st *State) error {
					if field == 0 {
						st.RequestSequence++
					} else {
						st.PublicationSequence++
					}
					return nil
				})
				if err != nil {
					errCh <- err
					return
				}
			}
			errCh <- nil
		}(i)
	}
	for range 2 {
		if e := <-errCh; e != nil {
			t.Fatal(e)
		}
	}
	st, e = s.Load(ctx)
	if e != nil || st.RequestSequence != 21 || st.PublicationSequence != 20 {
		t.Fatal("cross-process style stores lost independent updates", e)
	}
}

func TestRetentionAfter1000Requests(t *testing.T) {
	c, b, s := newFixtureCoordinator(t)
	c.PollInterval = time.Nanosecond
	// This measures retained state, not throughput. Leave time for race
	// instrumentation without changing any per-request execution budget.
	ctx, cancel := context.WithTimeout(context.Background(), 60*time.Second)
	defer cancel()
	for range 1000 {
		if _, e := c.Execute(ctx, []byte("return 42"), 1000); e != nil {
			t.Fatal(e)
		}
	}
	st, e := s.Load(ctx)
	if e != nil {
		t.Fatal(e)
	}
	p, _ := json.Marshal(st)
	if b.executions != 1000 || len(st.Intents) > 8 || len(p) > 16384 || len(st.Active.Source) != 0 || len(st.Active.Frames) != 0 {
		t.Fatal("unbounded retained current request", len(p), len(st.Intents))
	}
}

func TestUnknownControlDoesNotOverwriteLane(t *testing.T) {
	c, b, _ := newFixtureCoordinator(t)
	b.stalled = true
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	if _, e := c.Cancel(ctx, 2*time.Millisecond); !errors.Is(e, ErrPending) {
		t.Fatal(e)
	}
	before := len(b.writes)
	if _, e := c.Cancel(ctx, 2*time.Millisecond); !errors.Is(e, ErrPending) {
		t.Fatal(e)
	}
	if len(b.writes) != before {
		t.Fatal("overwrote unresolved cancel lane")
	}
	b.stalled = false
	if _, e := c.Disconnect(ctx, 100*time.Millisecond); e != nil {
		t.Fatal(e)
	}
}

func TestCorruptResultAndCleanupFailureBlockRelease(t *testing.T) {
	c, b, _ := newFixtureCoordinator(t)
	b.corruptResult = true
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	if _, e := c.Execute(ctx, []byte("return42"), 1000); e == nil {
		t.Fatal("accepted corrupt result")
	}
	for _, m := range b.writes {
		if m.Header.Kind == ResultAck {
			t.Fatal("ACK corrupt result")
		}
	}
	b.corruptResult = false
	b.afterWrite = func() {
		if b.writes[len(b.writes)-1].Header.Kind == ResultAck {
			b.box.ResourcesReleased = false
			b.afterWrite = nil
		}
	}
	shortCtx, shortCancel := context.WithTimeout(ctx, 5*time.Millisecond)
	defer shortCancel()
	st, e := c.Resume(shortCtx)
	if !errors.Is(e, ErrPending) || !st.Active.ResultSaved || st.Active.Released {
		t.Fatal("resource cleanup failure erased verified result or released", e)
	}
	b.box.ResourcesReleased = true
	if _, e = c.Resume(ctx); e != nil {
		t.Fatal(e)
	}
}

func TestTransferBudgetIndependentOfExecutionStart(t *testing.T) {
	c, b, _ := newFixtureCoordinator(t)
	b.stalled = true
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Millisecond)
	st, e := c.Execute(ctx, make([]byte, 4097), 1000)
	cancel()
	if !errors.Is(e, ErrPending) {
		t.Fatal(e)
	}
	ctx2, cancel2 := context.WithTimeout(context.Background(), time.Second)
	defer cancel2()
	c.Now = func() time.Time { return time.UnixMilli(int64(st.Active.Created) + 2000) }
	if _, e = c.Step(ctx2); !errors.Is(e, ErrPending) {
		t.Fatal("1-second execution budget incorrectly expired 2-second transfer", e)
	}
	if b.executions != 0 {
		t.Fatal("unprepared executed")
	}
	c.Now = func() time.Time { return time.UnixMilli(int64(st.Active.TransferDeadline) + 1) }
	if _, e = c.Step(ctx2); !errors.Is(e, ErrBudget) {
		t.Fatal("transfer deadline not bounded", e)
	}
	if _, e = c.Disconnect(ctx2, time.Millisecond); !errors.Is(e, ErrPending) {
		t.Fatal("cleanup budget mixed with transfer budget", e)
	}
}

func TestExactBusinessRejectionDoesNotBlockCleanupLane(t *testing.T) {
	for _, kind := range []Kind{Cancel, Close} {
		t.Run(kind.Lane(), func(t *testing.T) {
			c, b, _ := newFixtureCoordinator(t)
			b.reject = Commit
			ctx, cancel := context.WithTimeout(context.Background(), time.Second)
			defer cancel()
			if _, e := c.Execute(ctx, []byte("return 42"), 1000); !errors.Is(e, ErrRejected) {
				t.Fatal("rejected challenge remained pending", e)
			}
			if b.executions != 0 {
				t.Fatal("rejected executed")
			}
			var e error
			if kind == Cancel {
				_, e = c.Cancel(ctx, 100*time.Millisecond)
			} else {
				_, e = c.Disconnect(ctx, 100*time.Millisecond)
			}
			if e != nil {
				t.Fatal("business rejection blocked cleanup", e)
			}
			if kind == Cancel {
				st, e := c.Resume(ctx)
				if e != nil || !st.Active.Released || st.Active.Result.State != "cancelled" {
					t.Fatal("original commit rejection blocked cancellation result ACK/release", e)
				}
			}
		})
	}
}

func TestReloadPreparationDurableBeforeExactLease(t *testing.T) {
	c, b, s := newFixtureCoordinator(t)
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	st, e := c.Reload(ctx, 100*time.Millisecond)
	if e != nil {
		t.Fatal(e)
	}
	prepared := st.Intents["reload"]
	if !prepared.Accepted || prepared.Challenge == "" || b.reloads != 0 {
		t.Fatal("reload preparation executed or lacked durable challenge")
	}
	st, e = c.CommitReload(ctx, 100*time.Millisecond)
	if e != nil {
		t.Fatal(e)
	}
	lease := st.Intents["lease"]
	if !lease.Accepted || lease.Message.Header.Challenge != prepared.Challenge || hex.EncodeToString(lease.Message.Payload) != prepared.Message.Header.MessageID || b.reloads != 1 {
		t.Fatal("reload lease did not bind exact preparation")
	}
	if _, e = c.CommitReload(ctx, 100*time.Millisecond); e != nil || b.reloads != 1 {
		t.Fatal("reload lease replayed", e)
	}
	latest, _ := s.Load(ctx)
	if latest.Intents["reload"].Message.Header.MessageID != prepared.Message.Header.MessageID {
		t.Fatal("reload origin changed")
	}
}

func TestUnknownReloadLeaseNeverReplays(t *testing.T) {
	c, b, _ := newFixtureCoordinator(t)
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	if _, e := c.Reload(ctx, 100*time.Millisecond); e != nil {
		t.Fatal(e)
	}
	b.unknown = Lease
	if _, e := c.CommitReload(ctx, 100*time.Millisecond); e == nil {
		t.Fatal("expected unknown lease outcome")
	}
	before := len(b.writes)
	if _, e := c.CommitReload(ctx, 100*time.Millisecond); e != nil {
		t.Fatal(e)
	}
	if b.reloads != 1 || len(b.writes) != before {
		t.Fatal("unknown lease replayed")
	}
}

func TestRepairAtomicIntentAndNoWriteRecovery(t *testing.T) {
	c, b, s := newFixtureCoordinator(t)
	b.stalled = true
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Millisecond)
	st, e := c.Execute(ctx, []byte("return42"), 1000)
	cancel()
	if !errors.Is(e, ErrPending) {
		t.Fatal(e)
	}
	old := b.box.Arena
	b.box.Arena = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
	b.box.Repair = &RepairProof{PreviousArena: old, NewArena: b.box.Arena, Challenge: "cccccccccccccccccccccccccccccccc", RequestID: st.Active.RequestID, RequestSHA256: st.Active.Digest, NotStarted: true, LedgerRetained: true}
	b.stalled = false
	b.repairNoWrite = true
	ctx2, cancel2 := context.WithTimeout(context.Background(), time.Second)
	defer cancel2()
	if _, e = c.Repair(ctx2, true); e == nil {
		t.Fatal("expected proven no write")
	}
	durable, e := s.Load(ctx2)
	if e != nil {
		t.Fatal(e)
	}
	in := durable.Intents["bindResume"]
	if durable.Identity.Arena != b.box.Arena || durable.Active.Phase != "repairing" || in.Message.Header.Kind != Repair || in.Outcome.State != NoWrite {
		t.Fatal("identity changed without atomic repair intent")
	}
	b.repairNoWrite = false
	if _, e = c.Resume(ctx2); e != nil {
		t.Fatal(e)
	}
	if b.executions != 1 {
		t.Fatal("no-write repair failed or reran")
	}
}

func TestUnknownRepairNeverStartsFramesUntilExactReceipt(t *testing.T) {
	c, b, _ := newFixtureCoordinator(t)
	b.stalled = true
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Millisecond)
	st, e := c.Execute(ctx, []byte("return42"), 1000)
	cancel()
	if !errors.Is(e, ErrPending) {
		t.Fatal(e)
	}
	old := b.box.Arena
	b.box.Arena = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
	b.box.Repair = &RepairProof{PreviousArena: old, NewArena: b.box.Arena, Challenge: "cccccccccccccccccccccccccccccccc", RequestID: st.Active.RequestID, RequestSHA256: st.Active.Digest, NotStarted: true, LedgerRetained: true}
	b.unknown = Repair
	ctx2, cancel2 := context.WithTimeout(context.Background(), time.Second)
	defer cancel2()
	if _, e = c.Repair(ctx2, true); e == nil {
		t.Fatal("expected unknown")
	}
	before := len(b.writes)
	short, cancelShort := context.WithTimeout(ctx2, 5*time.Millisecond)
	defer cancelShort()
	if _, e = c.Resume(short); !errors.Is(e, ErrPending) {
		t.Fatal(e)
	}
	if len(b.writes) != before || b.executions != 0 {
		t.Fatal("unknown repair replayed or sent business before handshake")
	}
}

func idleRepairFixture(b *fixtureBackend) {
	old := b.box.Arena
	b.box.Arena = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
	b.box.Request = nil
	b.box.Terminal = nil
	b.box.ResourcesReleased = true
	b.box.Repair = &RepairProof{PreviousArena: old, NewArena: b.box.Arena, Challenge: "cccccccccccccccccccccccccccccccc", RequestID: "00000000000000000000000000000000", RequestSHA256: "0000000000000000000000000000000000000000000000000000000000000000", LedgerRetained: true, Idle: true, NoPendingRequest: true, ResourcesReleased: true}
}

func TestIdleRepairPreservesOwnerAndReleasedHistory(t *testing.T) {
	for _, released := range []bool{false, true} {
		t.Run(fmt.Sprint(released), func(t *testing.T) {
			c, b, s := newFixtureCoordinator(t)
			ctx, cancel := context.WithTimeout(context.Background(), time.Second)
			defer cancel()
			var last *ActiveRequest
			if released {
				st, e := c.Execute(ctx, []byte("return42"), 1000)
				if e != nil {
					t.Fatal(e)
				}
				last = st.Active
			}
			idleRepairFixture(b)
			if _, e := c.Repair(ctx, false); !errors.Is(e, ErrUnknown) {
				t.Fatal("accepted without host writer drain", e)
			}
			st, e := c.Repair(ctx, true)
			if e != nil {
				t.Fatal(e)
			}
			in := st.Intents["bindResume"]
			h := in.Message.Header
			if st.Identity.Runtime != fixtureIdentity.Runtime || st.Identity.Owner != fixtureIdentity.Owner || st.Identity.Session != fixtureIdentity.Session || st.Identity.Fence != fixtureIdentity.Fence || h.RequestSeq != 0 || h.BudgetMillis != 0 || h.TotalBytes != 0 || h.TransportAttempt != 1 || h.RequestID != strings.Repeat("0", 32) || h.RequestSHA256 != strings.Repeat("0", 64) {
				t.Fatal("idle repair changed logical ownership or created business")
			}
			st, e = c.Resume(ctx)
			if e != nil {
				t.Fatal(e)
			}
			if !st.Intents["bindResume"].Accepted || st.RepairProof == nil || !st.RepairProof.Idle {
				t.Fatal("idle handshake did not complete")
			}
			if last != nil && (st.Active.RequestID != last.RequestID || st.Active.Digest != last.Digest || st.Active.Sequence != last.Sequence || !st.Active.ResultSaved || !st.Active.Released || len(s.results) != 1) {
				t.Fatal("released result or tombstone history lost")
			}
			before := b.executions
			if _, e = c.Execute(ctx, []byte("return43"), 1000); e != nil {
				t.Fatal(e)
			}
			if b.executions != before+1 {
				t.Fatal("idle repair replayed historical business")
			}
		})
	}
}

func TestIdleRepairRejectsUnacknowledgedBusinessAndLedgerLoss(t *testing.T) {
	for _, bad := range []string{"active", "terminal", "resources", "ledger", "tombstone", "owner", "runtime"} {
		t.Run(bad, func(t *testing.T) {
			c, b, s := newFixtureCoordinator(t)
			ctx, cancel := context.WithTimeout(context.Background(), time.Second)
			defer cancel()
			if bad == "tombstone" {
				if _, e := c.Execute(ctx, []byte("return42"), 1000); e != nil {
					t.Fatal(e)
				}
			}
			idleRepairFixture(b)
			switch bad {
			case "active":
				_ = s.Update(ctx, func(st *State) error { st.Active = &ActiveRequest{RequestID: "pending", Released: false}; return nil })
			case "terminal":
				m, _ := resultFixtureForCore()
				b.box.Terminal = &m
			case "resources":
				b.box.ResourcesReleased = false
			case "ledger":
				b.box.Repair.LedgerRetained = false
			case "tombstone":
				b.box.Released.RequestSHA256 = strings.Repeat("0", 64)
			case "owner":
				b.box.Owner = "dddddddddddddddddddddddddddddddd"
			case "runtime":
				b.box.Runtime = "dddddddddddddddddddddddddddddddd"
			}
			before := len(b.writes)
			if _, e := c.Repair(ctx, true); !errors.Is(e, ErrUnknown) {
				t.Fatal("unsafe idle repair accepted", e)
			}
			if len(b.writes) != before {
				t.Fatal("unsafe proof produced an effect")
			}
		})
	}
}
func resultFixtureForCore() (ResultManifest, []byte) {
	p := []byte(`{}`)
	d := sha256.Sum256(p)
	h := hex.EncodeToString(d[:])
	return ResultManifest{RequestID: "66666666666666666666666666666666", RequestSHA256: strings.Repeat("0", 64), State: "success", SHA256: h, Bytes: uint32(len(p)), Pages: 1, PageSHA256: []string{h}}, p
}

func TestIdleRepairCrashCutsAndNoWriteRecovery(t *testing.T) {
	for _, cut := range []string{"intent_sync", "outcome_sync", "no_write", "unknown"} {
		t.Run(cut, func(t *testing.T) {
			c, b, s := newFixtureCoordinator(t)
			ctx, cancel := context.WithTimeout(context.Background(), time.Second)
			defer cancel()
			idleRepairFixture(b)
			before := len(b.writes)
			switch cut {
			case "intent_sync":
				s.failUpdate = true
			case "outcome_sync":
				b.afterWrite = func() { s.failUpdate = true; b.afterWrite = nil }
			case "no_write":
				b.repairNoWrite = true
			case "unknown":
				b.stalled = true
				b.unknown = Repair
			}
			if _, e := c.Repair(ctx, true); e == nil {
				t.Fatal("cut did not interrupt repair")
			}
			st, e := s.Load(ctx)
			if e != nil {
				t.Fatal(e)
			}
			if cut == "intent_sync" {
				if st.Identity != fixtureIdentity || len(b.writes) != before {
					t.Fatal("identity/effect escaped failed atomic intent")
				}
				return
			}
			in := st.Intents["bindResume"]
			if st.Identity.Arena != b.box.Arena || st.RepairProof == nil || !st.RepairProof.Idle || in.Message.Header.Kind != Repair || st.Active != nil {
				t.Fatal("generation and idle repair intent were not atomic")
			}
			if cut == "unknown" {
				before = len(b.writes)
				if _, e = c.Execute(ctx, []byte("return42"), 1000); !errors.Is(e, ErrPending) {
					t.Fatal("pending handshake admitted new request", e)
				}
				short, stop := context.WithTimeout(ctx, 5*time.Millisecond)
				defer stop()
				if _, e = c.Resume(short); !errors.Is(e, ErrPending) {
					t.Fatal(e)
				}
				st, _ = s.Load(ctx)
				if len(b.writes) != before || st.Active != nil {
					t.Fatal("unknown idle repair replayed or allocated business")
				}
				return
			}
			b.repairNoWrite = false
			if _, e = c.Resume(ctx); e != nil {
				t.Fatal(e)
			}
			if b.executions != 0 {
				t.Fatal("idle recovery created an execution")
			}
		})
	}
}

func TestCurrentActorReadinessRequiredButDoesNotBlockCleanup(t *testing.T) {
	c, b, _ := newFixtureCoordinator(t)
	b.box.ActorGUID = "retained-old-guid"
	b.box.ActorReady = false
	if _, e := EncodeSendbox(b.box); e == nil {
		t.Fatal("businessReady with unavailable current actor accepted")
	}
	b.box.BusinessReady = false
	b.box.Ready = false
	wire, e := EncodeSendbox(b.box)
	if e != nil {
		t.Fatal(e)
	}
	box, e := DecodeSendbox(wire)
	if e != nil || box.ActorReady {
		t.Fatal("retained GUID was treated as readiness", e)
	}
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	if _, e = c.Execute(ctx, []byte("return42"), 1000); !errors.Is(e, ErrBusy) {
		t.Fatal("unavailable actor allowed business", e)
	}
	if _, e = c.Disconnect(ctx, 100*time.Millisecond); e != nil {
		t.Fatal("actor loss blocked exact cleanup control", e)
	}
}
