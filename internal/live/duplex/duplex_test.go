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
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"
)

var fixtureIdentity = Identity{Runtime: "11111111111111111111111111111111", Arena: "22222222222222222222222222222222", Session: "33333333333333333333333333333333", Owner: "44444444444444444444444444444444", ActorBinding: "55555555555555555555555555555555", Fence: 1}

func TestMailboxV1RejectsPreviousSchemaAndMagic(t *testing.T) {
	frames, err := NewFrames(fixtureIdentity, strings.Repeat("6", 32), 1, 1, 1234, 1000, []byte("return true"))
	if err != nil {
		t.Fatal(err)
	}
	wire, err := EncodeMessage(frames[0])
	if err != nil {
		t.Fatal(err)
	}
	copy(wire, "LYCDPX01")
	if _, err := DecodeMessage(wire); err == nil {
		t.Fatal("accepted retired duplex header")
	}
	box := fixtureSendbox()
	box.Schema = "lycheedev.duplex.v1"
	if _, err := EncodeSendbox(box); err == nil {
		t.Fatal("accepted retired duplex schema")
	}
	box.Schema = "lycheedev.mailbox.v1"
	wire, err = EncodeSendbox(box)
	if err != nil {
		t.Fatal(err)
	}
	copy(wire, "LYCSBX01")
	if _, err := DecodeSendbox(wire); err == nil {
		t.Fatal("accepted retired duplex sendbox")
	}
	for _, layout := range []string{"", "256-row-v1"} {
		box.LayoutID = layout
		if _, err := EncodeSendbox(box); err == nil {
			t.Fatalf("accepted incompatible layout %q", layout)
		}
	}
}

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
			if len(frames) != 1 || frames[0].Header.FrameCount != 1 || frames[0].Header.FrameIndex != 1 {
				t.Fatal("command must have exactly one publication")
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
	if before != "f23f71e94dabb63d2de798fa76a023b15d1d0c7f3fb6ffd0c124fa4862d05a22" {
		t.Fatal("request golden mismatch", before)
	}
	frames[0].Header.MessageID = "77777777777777777777777777777777"
	wire, e = EncodeMessage(frames[0])
	if e != nil {
		t.Fatal(e)
	}
	if hex.EncodeToString(wire[232:264]) != strings.Repeat("0", 64) {
		t.Fatal("initial ACK is nonzero")
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

func TestSendboxAcceptsAddonRequestTotalBytes(t *testing.T) {
	s := fixtureSendbox()
	s.Identity = fixtureIdentity
	s.Phase, s.Ready, s.BusinessReady = "running", false, false
	s.Request = &RequestState{RequestID: strings.Repeat("6", 32), RequestSHA256: strings.Repeat("7", 64), RequestSeq: 1, TransportAttempt: 1, AcceptedFrames: []uint32{1}}
	wire, err := EncodeSendbox(s)
	if err != nil {
		t.Fatal(err)
	}
	var fields map[string]any
	if err := json.Unmarshal(wire[44:], &fields); err != nil {
		t.Fatal(err)
	}
	fields["request"].(map[string]any)["totalBytes"] = 149
	payload, err := json.Marshal(fields)
	if err != nil {
		t.Fatal(err)
	}
	w := make([]byte, 44+len(payload))
	copy(w, "LYCMSB01")
	binary.LittleEndian.PutUint32(w[8:12], uint32(len(payload)))
	digest := sha256.Sum256(payload)
	copy(w[12:44], digest[:])
	copy(w[44:], payload)
	out, err := DecodeSendbox(w)
	if err != nil || out.Request == nil {
		t.Fatalf("addon request projection rejected: %v", err)
	}
	projection, _ := json.Marshal(out.Request)
	if !strings.Contains(string(projection), `"totalBytes":149`) {
		t.Fatal("addon request length was lost")
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
	id := fixtureIdentity
	id.Owner = zeroToken
	id.Session = zeroToken
	id.Fence = 0
	return Sendbox{Identity: id, Schema: "lycheedev.mailbox.v1", LayoutID: MailboxLayoutID, Phase: "ready_unbound", Ready: true, ActorReady: true, TransportReady: true, BusinessReady: true, ControlReady: true, StatusSequence: 1, Heartbeat: 1, ResourcesReleased: true, ReadyChallenge: strings.Repeat("a", 32), AdmissionSequence: 1, Receipts: map[string]Receipt{}}
}

type fixtureBackend struct {
	box           Sendbox
	store         *fixtureStore
	journal       Store
	writes        []Message
	executions    int
	stalled       bool
	running       bool
	unknown       Kind
	corruptResult bool
	result        []byte
	afterWrite    func()
}

func (b *fixtureBackend) Observe(context.Context) (Sendbox, error) {
	b.box.Heartbeat++
	b.box.StatusSequence++
	p, _ := json.Marshal(b.box)
	var out Sendbox
	_ = json.Unmarshal(p, &out)
	return out, nil
}
func (b *fixtureBackend) Publish(ctx context.Context, m Message) (WriteOutcome, error) {
	journal := Store(b.store)
	if b.journal != nil {
		journal = b.journal
	}
	st, _ := journal.Load(ctx)
	in, ok := st.Intents[m.Header.Kind.Lane()]
	if !ok || in.Message.Header.MessageID != m.Header.MessageID || in.Outcome.State != UnknownWrite {
		return WriteOutcome{State: NoWrite}, errors.New("intent not durable before effect")
	}
	wire, e := EncodeMessage(m)
	if e != nil {
		return WriteOutcome{State: NoWrite}, e
	}
	b.writes = append(b.writes, m)
	h := m.Header
	if !b.stalled {
		switch h.Kind {
		case Frame:
			if e = ValidateFrameTransition(b.box, m); e != nil {
				return WriteOutcome{State: NoWrite}, e
			}
			if b.box.Terminal != nil {
				b.box.Released = &Released{b.box.Terminal.RequestID, b.box.Terminal.RequestSHA256}
			}
			b.box.Identity = Identity{h.Runtime, h.Arena, h.Session, h.Owner, h.ActorBinding, h.Fence}
			b.box.Terminal = nil
			b.box.Request = &RequestState{RequestID: h.RequestID, RequestSHA256: h.RequestSHA256, RequestSeq: h.RequestSeq, TransportAttempt: h.TransportAttempt, AcceptedFrames: []uint32{1}, Challenge: h.Challenge}
			b.box.ClosedAdmission = false
			b.box.Phase = "running"
			b.box.Ready = false
			b.box.BusinessReady = false
			b.box.ReadyChallenge = ""
			b.box.ResourcesReleased = false
			b.executions++
			if !b.running {
				b.terminal(h, "success", []byte(`{"value":42}`), true)
			}
		case Repair:
			b.box.Repair = nil
			b.box.Ready = true
			b.box.BusinessReady = true
			b.box.ReadyChallenge = strings.Repeat("f", 32)
			b.box.ResourcesReleased = true
			b.receipt(m, "repaired")
		case Reload:
			if b.box.Terminal != nil || b.box.Request != nil {
				return WriteOutcome{State: NoWrite}, ErrBusy
			}
			b.box.Identity = Identity{h.Runtime, h.Arena, h.Session, h.Owner, h.ActorBinding, h.Fence}
			b.box.Ready = false
			b.box.BusinessReady = false
			b.box.ReadyChallenge = ""
			b.receipt(m, "accepted")
			r := b.box.Receipts["stop"]
			r.Challenge = strings.Repeat("d", 32)
			b.box.Receipts["stop"] = r
		case Lease:
			prepared := st.ReloadPrepared
			if prepared == nil || !prepared.Accepted || prepared.Challenge != h.Challenge || hex.EncodeToString(m.Payload) != prepared.Message.Header.MessageID {
				return WriteOutcome{State: NoWrite}, ErrIdentity
			}
			b.receipt(m, "lease_observed")
		case Cancel, Close:
			if h.Kind == Cancel && len(m.Payload) == 92 && b.box.Terminal != nil && b.box.Terminal.RequestID != h.RequestID {
				expected, _ := EncodeResultAck(*b.box.Terminal)
				if string(expected) != string(m.Payload) || b.box.ReadyChallenge != h.Challenge {
					return WriteOutcome{State: NoWrite}, ErrIdentity
				}
				b.box.Released = &Released{b.box.Terminal.RequestID, b.box.Terminal.RequestSHA256}
				b.box.Terminal = nil
				b.box.Request = nil
			}
			if b.box.Terminal == nil && h.RequestID != zeroToken {
				b.box.Identity = Identity{h.Runtime, h.Arena, h.Session, h.Owner, h.ActorBinding, h.Fence}
				started := b.box.Request != nil && !b.box.Request.NotStarted
				b.box.Request = &RequestState{RequestID: h.RequestID, RequestSHA256: h.RequestSHA256, RequestSeq: h.RequestSeq, TransportAttempt: h.TransportAttempt, AcceptedFrames: []uint32{1}, Challenge: h.Challenge, NotStarted: !started}
				b.terminal(h, "cancelled", []byte(`{"cancelled":true}`), started)
			}
			if h.Kind == Close && b.box.Terminal != nil && len(m.Payload) == 92 {
				expected, _ := EncodeResultAck(*b.box.Terminal)
				if string(expected) != string(m.Payload) || st.Active == nil || !st.Active.ResultSaved {
					return WriteOutcome{State: NoWrite}, errors.New("ACK before exact durable result")
				}
				b.box.Released = &Released{h.RequestID, h.RequestSHA256}
				b.box.Terminal = nil
				b.box.Request = nil
				b.box.Phase = "closed"
				b.box.Ready = false
				b.box.BusinessReady = false
				b.box.ReadyChallenge = ""
				b.receipt(m, "closed")
			} else if h.Kind == Close && h.RequestID == zeroToken {
				b.box.Identity = Identity{h.Runtime, h.Arena, h.Session, h.Owner, h.ActorBinding, h.Fence}
				b.box.Phase = "closed"
				b.box.Ready = false
				b.box.BusinessReady = false
				b.box.ReadyChallenge = ""
				b.receipt(m, "closed")
			} else if h.Kind == Close {
				b.box.Phase = "closing"
				b.box.Ready = false
				b.box.BusinessReady = false
				b.box.ReadyChallenge = ""
				b.receipt(m, "closing")
			} else {
				b.receipt(m, "not_started")
			}
		}
	}
	if b.afterWrite != nil {
		b.afterWrite()
	}
	if b.unknown == h.Kind {
		return WriteOutcome{State: UnknownWrite, Bytes: uint64(len(wire))}, ErrUnknown
	}
	return WriteOutcome{State: CompleteWrite, Bytes: uint64(len(wire)), ReadbackVerified: true}, nil
}
func (b *fixtureBackend) receipt(m Message, state string) {
	h := m.Header
	b.box.Receipts["stop"] = Receipt{MessageID: h.MessageID, RequestID: h.RequestID, RequestSHA256: h.RequestSHA256, State: state}
}
func (b *fixtureBackend) terminal(h Header, state string, p []byte, started bool) {
	sum := sha256.Sum256(p)
	d := hex.EncodeToString(sum[:])
	b.result = append([]byte(nil), p...)
	effects := "none_started"
	if started {
		effects = "may_have_occurred"
	}
	b.box.Terminal = &ResultManifest{RequestID: h.RequestID, RequestSHA256: h.RequestSHA256, State: state, SHA256: d, Bytes: uint32(len(p)), Pages: 1, PageSHA256: []string{d}, ExecutionStarted: started, Effects: effects, ResourcesReleased: true}
	b.box.Phase = "result_pending"
	b.box.ResourcesReleased = true
	b.box.Ready = true
	b.box.BusinessReady = true
	b.box.AdmissionSequence++
	b.box.ReadyChallenge = fmt.Sprintf("%032x", b.box.AdmissionSequence)
}
func (b *fixtureBackend) ReadResult(context.Context, ResultManifest) ([]byte, error) {
	p := append([]byte(nil), b.result...)
	if b.corruptResult {
		p = append(p, 1)
	}
	return p, nil
}
func (b *fixtureBackend) Close(context.Context) error { return nil }
func newFixtureCoordinator(t *testing.T) (*Coordinator, *fixtureBackend, *fixtureStore) {
	t.Helper()
	s := newFixtureStore()
	b := &fixtureBackend{box: fixtureSendbox(), store: s}
	c := NewCoordinator(b, s)
	c.PollInterval = time.Microsecond
	if _, e := c.Connect(context.Background(), fixtureIdentity); e != nil {
		t.Fatal(e)
	}
	return c, b, s
}

func TestExecuteOneWriteAndRetainedResult(t *testing.T) {
	c, b, s := newFixtureCoordinator(t)
	ctx := context.Background()
	st, e := c.Execute(ctx, []byte("return 42"), 1000)
	if e != nil || !st.Bound || !st.Active.ResultSaved || st.Active.Released || len(b.writes) != 1 || b.executions != 1 {
		t.Fatal("single publication did not retain exact durable result", e)
	}
	first := st.Active.RequestID
	previous := *st.Active.Result
	st, e = c.Execute(ctx, []byte("return 43"), 1000)
	ack, _ := ResultAckSHA256(previous)
	if e != nil || len(b.writes) != 2 || b.writes[1].Header.PreviousResultAckSHA != ack || b.box.Released.RequestID != first || len(s.results) != 2 {
		t.Fatal("next command did not jointly ACK exact prior result", e)
	}
	st, e = c.Disconnect(ctx, time.Second)
	if e != nil || !st.Closed || !st.Active.Released || len(b.writes) != 3 || b.writes[2].Header.Kind != Close || len(b.writes[2].Payload) != 92 {
		t.Fatal("final ACK/close not exact", e)
	}
	if e = ValidateState(st); e != nil {
		t.Fatal(e)
	}
}
func TestConnectDoesNotPublishOrClaimAddonBinding(t *testing.T) {
	c, b, _ := newFixtureCoordinator(t)
	st, e := c.Store.Load(context.Background())
	if e != nil || !st.Selected || st.Bound || len(b.writes) != 0 {
		t.Fatal("connect claimed addon acceptance", e)
	}
}
func TestUnknownCommandRecoversExactTerminalWithoutReplay(t *testing.T) {
	c, b, _ := newFixtureCoordinator(t)
	b.unknown = Frame
	ctx := context.Background()
	if _, e := c.Execute(ctx, []byte("return 42"), 1000); !errors.Is(e, ErrUnknown) {
		t.Fatal(e)
	}
	b.unknown = 0
	st, e := c.Resume(ctx)
	if e != nil || !st.Active.ResultSaved || len(b.writes) != 1 || b.executions != 1 {
		t.Fatal("unknown write replayed", e)
	}
}
func TestUnknownUnacceptedCommandNeverReplays(t *testing.T) {
	c, b, _ := newFixtureCoordinator(t)
	b.stalled = true
	b.unknown = Frame
	ctx := context.Background()
	if _, e := c.Execute(ctx, []byte("return 42"), 1000); !errors.Is(e, ErrUnknown) {
		t.Fatal(e)
	}
	for range 3 {
		if _, e := c.Step(ctx); !errors.Is(e, ErrPending) {
			t.Fatal(e)
		}
	}
	if len(b.writes) != 1 || b.executions != 0 {
		t.Fatal("unknown command resent")
	}
	b.stalled = false
	b.unknown = 0
	st, e := c.Cancel(ctx, time.Second)
	if e != nil {
		t.Fatal(e)
	}
	st, e = c.Resume(ctx)
	if e != nil || !st.Active.ResultSaved || st.Active.Result.ExecutionStarted || st.Active.Result.State != "cancelled" {
		t.Fatal("preaccept cancel missing exact not_started", e)
	}
	st, e = c.Disconnect(ctx, time.Second)
	if e != nil || !st.Closed {
		t.Fatal(e)
	}
}
func TestPersistenceBeforeEffectAndResultACK(t *testing.T) {
	c, b, s := newFixtureCoordinator(t)
	s.failUpdate = true
	if _, e := c.Execute(context.Background(), []byte("return42"), 1000); e == nil || len(b.writes) != 0 {
		t.Fatal("effect preceded durable intent")
	}
	s.failResult = true
	if _, e := c.Execute(context.Background(), []byte("return42"), 1000); !errors.Is(e, ErrPersistence) {
		t.Fatal(e)
	}
	before := len(b.writes)
	if _, e := c.Disconnect(context.Background(), time.Second); !errors.Is(e, ErrPersistence) || len(b.writes) != before {
		t.Fatal("ACK preceded result durability", e)
	}
	s.failResult = false
	if _, e := c.Resume(context.Background()); e != nil {
		t.Fatal(e)
	}
}
func TestStopUncertainIntentSerializesCancelAndClose(t *testing.T) {
	c, b, _ := newFixtureCoordinator(t)
	b.running = true
	short, cancel := context.WithTimeout(context.Background(), time.Millisecond)
	_, e := c.Execute(short, []byte("return42"), 1000)
	cancel()
	if !errors.Is(e, ErrPending) {
		t.Fatal(e)
	}
	b.stalled = true
	_, e = c.Cancel(context.Background(), time.Millisecond)
	if !errors.Is(e, ErrPending) {
		t.Fatal(e)
	}
	before := len(b.writes)
	_, e = c.Disconnect(context.Background(), time.Millisecond)
	if !errors.Is(e, ErrPending) || len(b.writes) != before {
		t.Fatal("uncertain cancel overwritten by close", e)
	}
	b.stalled = false
	b.receipt(b.writes[len(b.writes)-1], "cancel_requested")
	st, e := c.Disconnect(context.Background(), time.Second)
	if e != nil || !st.Closed || !st.Active.ResultSaved || !st.Active.Released {
		t.Fatal("running close did not collect+final ACK", e)
	}
}
func TestRuntimeChangeRetainsUnknownEvidence(t *testing.T) {
	c, b, s := newFixtureCoordinator(t)
	b.stalled = true
	b.unknown = Frame
	_, _ = c.Execute(context.Background(), []byte("return42"), 1000)
	before, _ := s.Load(context.Background())
	b.box.Runtime = strings.Repeat("b", 32)
	if _, e := c.Step(context.Background()); !errors.Is(e, ErrIdentity) {
		t.Fatal(e)
	}
	after, _ := s.Load(context.Background())
	if after.Active.RequestID != before.Active.RequestID || len(b.writes) != 1 {
		t.Fatal("runtime change replaced evidence")
	}
}
func TestSequentialCommandsBoundedCurrentState(t *testing.T) {
	c, b, s := newFixtureCoordinator(t)
	for range 140 {
		if _, e := c.Execute(context.Background(), []byte("return42"), 1000); e != nil {
			t.Fatal(e)
		}
	}
	st, _ := s.Load(context.Background())
	p, _ := json.Marshal(st)
	if b.executions != 140 || len(b.writes) != 140 || len(st.Intents) != 1 || len(p) > 16384 || len(s.results) != 140 {
		t.Fatal("unbounded state or repeated publication", len(p))
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
		st.Intents[Frame.Lane()] = Intent{Message: frames[0], Outcome: WriteOutcome{State: UnknownWrite}}
		return nil
	}); e != nil {
		t.Fatal("maximum source journal rejected", e)
	}
	st, e := s.Load(ctx)
	if e != nil || len(st.Active.Source) != MaxSourceBytes || len(st.Active.Frames) != 1 {
		t.Fatal("maximum journal did not roundtrip", e)
	}
	if in := st.Intents[Frame.Lane()]; len(in.Message.Payload) != MaxSourceBytes || in.Outcome.State != UnknownWrite {
		t.Fatal("maximum outstanding publication intent did not roundtrip")
	}
	if p, err := os.ReadFile(filepath.Join(dir, "state.json")); err != nil || len(p) <= MaxSourceBytes || len(p) > 2*MaxSourceBytes {
		t.Fatal("maximum pending command journal bound", len(p), err)
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

func TestReloadPreparationAndLeaseShareStopWithoutReplay(t *testing.T) {
	c, b, _ := newFixtureCoordinator(t)
	if _, e := c.Execute(context.Background(), []byte("return42"), 1000); e != nil {
		t.Fatal(e)
	}
	st, e := c.Reload(context.Background(), time.Second)
	if e != nil || st.ReloadPrepared == nil || !st.ReloadPrepared.Accepted || !st.Active.Released {
		t.Fatal("reload not prepared after exact final ACK", e)
	}
	if len(b.writes) != 3 || b.writes[1].Header.Kind != Close || b.writes[2].Header.Kind != Reload {
		t.Fatal("reload publication order")
	}
	b.unknown = Lease
	if _, e = c.CommitReload(context.Background(), time.Second); !errors.Is(e, ErrUnknown) {
		t.Fatal(e)
	}
	before := len(b.writes)
	b.unknown = 0
	st, e = c.CommitReload(context.Background(), time.Second)
	if e != nil || len(b.writes) != before || !st.Intents["stop"].Accepted {
		t.Fatal("exact lease replayed", e)
	}
}
func TestUnknownReloadLeaseNotResent(t *testing.T) {
	c, b, _ := newFixtureCoordinator(t)
	if _, e := c.Execute(context.Background(), []byte("return42"), 1000); e != nil {
		t.Fatal(e)
	}
	if _, e := c.Reload(context.Background(), time.Second); e != nil {
		t.Fatal(e)
	}
	b.stalled = true
	b.unknown = Lease
	if _, e := c.CommitReload(context.Background(), time.Second); !errors.Is(e, ErrUnknown) {
		t.Fatal(e)
	}
	before := len(b.writes)
	b.unknown = 0
	if _, e := c.CommitReload(context.Background(), time.Millisecond); !errors.Is(e, ErrPending) || len(b.writes) != before {
		t.Fatal("unknown reload lease overwritten", e)
	}
}
func TestIdleRepairRequiresDrainAndDoesNotReplayHistory(t *testing.T) {
	c, b, s := newFixtureCoordinator(t)
	if _, e := c.Execute(context.Background(), []byte("return42"), 1000); e != nil {
		t.Fatal(e)
	}
	if _, e := c.Disconnect(context.Background(), time.Second); e != nil {
		t.Fatal(e)
	}
	b.box.Phase = "ready_unbound"
	b.box.ClosedAdmission = true
	old := b.box.Arena
	b.box.Arena = strings.Repeat("b", 32)
	b.box.Ready = false
	b.box.BusinessReady = false
	b.box.ReadyChallenge = ""
	b.box.Repair = &RepairProof{PreviousArena: old, NewArena: b.box.Arena, Challenge: strings.Repeat("c", 32), RequestID: zeroToken, RequestSHA256: zeroDigest, LedgerRetained: true, Idle: true, NoPendingRequest: true, ResourcesReleased: true}
	if _, e := c.Repair(context.Background(), false); !errors.Is(e, ErrUnknown) {
		t.Fatal("drain not required", e)
	}
	if _, e := c.Repair(context.Background(), true); e != nil {
		t.Fatal(e)
	}
	st, e := c.Resume(context.Background())
	if e != nil || st.RepairProof == nil || st.Identity.Arena != b.box.Arena || b.executions != 1 {
		t.Fatal("idle repair replayed historical business", e)
	}
	if e = ValidateState(st); e != nil {
		t.Fatal(e)
	}
	if len(s.results) != 1 {
		t.Fatal("repair lost historical result")
	}
}
func TestUnboundDisconnectAndFileJournalRoundtrip(t *testing.T) {
	c, b, _ := newFixtureCoordinator(t)
	st, e := c.Disconnect(context.Background(), time.Second)
	if e != nil || st.Closed || !st.LocalRetired || len(b.writes) != 0 {
		t.Fatal("unbound session could not retire", e)
	}
	file := NewFileStore(t.TempDir())
	c, b, _ = newFixtureCoordinator(t)
	if e = file.Update(context.Background(), func(target *State) error { *target = b.store.state; return nil }); e != nil {
		t.Fatal(e)
	}
	c.Store = file
	b.journal = file
	if st, e = c.Execute(context.Background(), []byte("return42"), 1000); e != nil || !st.Active.ResultSaved {
		t.Fatal(e)
	}
	// Fresh coordinator resumes solely from compacted headers and one source.
	c = NewCoordinator(b, NewFileStore(file.Dir))
	c.PollInterval = time.Microsecond
	if _, e = c.Execute(context.Background(), []byte("return43"), 1000); e != nil {
		t.Fatal(e)
	}
	if st, e = c.Disconnect(context.Background(), time.Second); e != nil || !st.Active.Released || !st.Closed {
		t.Fatal(e)
	}
	restarted, e := NewFileStore(file.Dir).Load(context.Background())
	if e != nil || len(restarted.Active.Source) != 0 || len(restarted.Intents) != 1 {
		t.Fatal("final source/command release not durable", e)
	}
}

func TestReplacementUnknownCancelCarriesOriginalExactPreviousACK(t *testing.T) {
	for _, accepted := range []bool{false, true} {
		t.Run(fmt.Sprint(accepted), func(t *testing.T) {
			c, b, _ := newFixtureCoordinator(t)
			first, e := c.Execute(context.Background(), []byte("return42"), 1000)
			if e != nil {
				t.Fatal(e)
			}
			previousACK, _ := EncodeResultAck(*first.Active.Result)
			b.stalled = !accepted
			b.running = true
			b.unknown = Frame
			if _, e = c.Execute(context.Background(), []byte("return43"), 1000); !errors.Is(e, ErrUnknown) {
				t.Fatal(e)
			}
			b.stalled = false
			b.unknown = 0
			if _, e = c.Cancel(context.Background(), time.Second); e != nil {
				t.Fatal(e)
			}
			stop := b.writes[len(b.writes)-1]
			if stop.Header.Kind != Cancel || string(stop.Payload) != string(previousACK) {
				t.Fatal("cancel did not retain journal's exact previous ACK")
			}
			st, e := c.Resume(context.Background())
			if e != nil || !st.Active.ResultSaved || st.Active.Result.ExecutionStarted != accepted || st.Active.Result.State != "cancelled" {
				t.Fatal("replacement cancellation changed execution fact", e)
			}
			if _, e = c.Disconnect(context.Background(), time.Second); e != nil {
				t.Fatal(e)
			}
		})
	}
}
func TestDisconnectRevokesUnknownReplacementThenFinallyACKsNewResult(t *testing.T) {
	c, b, _ := newFixtureCoordinator(t)
	first, e := c.Execute(context.Background(), []byte("return42"), 1000)
	if e != nil {
		t.Fatal(e)
	}
	prior, _ := EncodeResultAck(*first.Active.Result)
	b.stalled = true
	b.unknown = Frame
	if _, e = c.Execute(context.Background(), []byte("return43"), 1000); !errors.Is(e, ErrUnknown) {
		t.Fatal(e)
	}
	b.stalled = false
	b.unknown = 0
	st, e := c.Disconnect(context.Background(), time.Second)
	if e != nil || !st.Active.Released || !st.Closed || !st.Closing || st.Active.Result.ExecutionStarted {
		t.Fatal("unknown replacement close failed", e)
	}
	if len(b.writes) != 4 || b.writes[2].Header.Kind != Cancel || string(b.writes[2].Payload) != string(prior) || b.writes[3].Header.Kind != Close || len(b.writes[3].Payload) != 92 {
		t.Fatal("ambiguous priorACK treated as finalclose")
	}
}

func TestFileStoreInspectionRestoresCompactedCommandWithoutLease(t *testing.T) {
	ctx := context.Background()
	c, b, fixture := newFixtureCoordinator(t)
	file := NewFileStore(t.TempDir())
	if e := file.Update(ctx, func(st *State) error { *st = fixture.state; return nil }); e != nil {
		t.Fatal(e)
	}
	c.Store = file
	b.journal = file
	source := []byte("return 42 -- nonempty persisted command")
	first, e := c.Execute(ctx, source, 1000)
	if e != nil {
		t.Fatal(e)
	}
	path := filepath.Join(file.Dir, "state.json")
	before, e := os.ReadFile(path)
	if e != nil {
		t.Fatal(e)
	}
	var compact State
	if e = json.Unmarshal(before, &compact); e != nil {
		t.Fatal(e)
	}
	if len(compact.Active.Source) != len(source) || compact.Active.Frames[0].Payload != nil || compact.Intents["command"].Message.Payload != nil {
		t.Fatal("fixture is not a source-once compact journal")
	}
	unlock, e := file.lock(ctx)
	if e != nil {
		t.Fatal(e)
	}
	limited, cancel := context.WithTimeout(ctx, 50*time.Millisecond)
	inspected, e := NewFileStore(file.Dir).Inspect(limited)
	cancel()
	unlock()
	if e != nil || string(inspected.Active.Frames[0].Payload) != string(source) || string(inspected.Intents["command"].Message.Payload) != string(source) || !inspected.Active.ResultSaved {
		t.Fatal("read-only inspection did not restore exact active bytes independently of journal lease", e)
	}
	after, e := os.ReadFile(path)
	if e != nil || string(before) != string(after) {
		t.Fatal("inspection changed compact journal", e)
	}
	if e = ValidateState(inspected); e != nil {
		t.Fatal(e)
	}
	status, box, e := c.Status(ctx)
	if e != nil || status.Active.RequestID != first.Active.RequestID || box.Terminal == nil {
		t.Fatal("status rejected real compact journal", e)
	}
	restarted := NewCoordinator(b, NewFileStore(file.Dir))
	restarted.PollInterval = time.Microsecond
	next, e := restarted.Execute(ctx, []byte("return 43"), 1000)
	if e != nil || !next.Active.ResultSaved || next.Active.Sequence != 2 || len(b.writes) != 2 {
		t.Fatal("next drive rejected restored command", e)
	}
	expected, _ := ResultAckSHA256(*first.Active.Result)
	if b.writes[1].Header.PreviousResultAckSHA != expected {
		t.Fatal("inspection/restart changed previous result ACK")
	}
}

func TestClosedReconnectionIdleDisconnectDoesNotWriteGame(t *testing.T) {
	old, b, _ := newFixtureCoordinator(t)
	if _, e := old.Execute(context.Background(), []byte("return42"), 1000); e != nil {
		t.Fatal(e)
	}
	if _, e := old.Disconnect(context.Background(), time.Second); e != nil {
		t.Fatal(e)
	}
	b.box.Phase = "ready_unbound"
	b.box.ClosedAdmission = true
	b.box.Ready = true
	b.box.BusinessReady = true
	b.box.ReadyChallenge = strings.Repeat("e", 32)
	b.box.AdmissionSequence++
	id, e := NextIdentity(b.box, strings.Repeat("b", 32), strings.Repeat("c", 32))
	if e != nil {
		t.Fatal(e)
	}
	next := NewCoordinator(b, newFixtureStore())
	if _, e = next.Connect(context.Background(), id); e != nil {
		t.Fatal(e)
	}
	before := len(b.writes)
	if _, e = next.Reload(context.Background(), time.Second); !errors.Is(e, ErrBusy) {
		t.Fatal("unbound reload tried to bind session", e)
	}
	st, e := next.Disconnect(context.Background(), time.Second)
	if e != nil || !st.LocalRetired || st.Closed || st.Bound || len(b.writes) != before || b.box.Owner != fixtureIdentity.Owner {
		t.Fatal("idle new selection published invalid seq0 close", e)
	}
}
func TestValidatingProjectionKeepsFirstOwnerUnbound(t *testing.T) {
	for _, closed := range []bool{false, true} {
		t.Run(fmt.Sprint(closed), func(t *testing.T) {
			c, b, _ := newFixtureCoordinator(t)
			if closed {
				b.box.Identity = fixtureIdentity
				b.box.ClosedAdmission = true
				b.box.Released = &Released{strings.Repeat("7", 32), strings.Repeat("0", 64)}
				id, e := NextIdentity(b.box, strings.Repeat("b", 32), strings.Repeat("c", 32))
				if e != nil {
					t.Fatal(e)
				}
				freshStore := newFixtureStore()
				b.store = freshStore
				c = NewCoordinator(b, freshStore)
				if _, e = c.Connect(context.Background(), id); e != nil {
					t.Fatal(e)
				}
			}
			b.stalled = true
			b.unknown = Frame
			_, e := c.Execute(context.Background(), []byte("return42"), 1000)
			if !errors.Is(e, ErrUnknown) {
				t.Fatal(e)
			}
			m := b.writes[len(b.writes)-1]
			b.box.Phase = "validating"
			b.box.Ready = false
			b.box.BusinessReady = false
			b.box.Validation = &ValidationProgress{RequestID: m.Header.RequestID, RequestSHA256: m.Header.RequestSHA256, TotalBytes: uint32(len(m.Payload)), CopiedBytes: 4, HashedBytes: 2}
			wire, e := EncodeSendbox(b.box)
			if e != nil {
				t.Fatal(e)
			}
			box, e := DecodeSendbox(wire)
			if e != nil || box.Validation.CopiedBytes != 4 {
				t.Fatal("validation projection not interoperable", e)
			}
			st, e := c.Step(context.Background())
			if !errors.Is(e, ErrPending) || st.Bound || st.Active.ExecutionObservedHostAt != 0 || st.Active.Phase != "validating" {
				t.Fatal("validation inferred accepted/executed or identity changed", e)
			}
			status, _, e := c.Status(context.Background())
			if e != nil || status.Bound {
				t.Fatal(e)
			}
		})
	}
}

func TestValidationUsesTransferDeadlineNotExecutionBudget(t *testing.T) {
	c, b, _ := newFixtureCoordinator(t)
	b.stalled = true
	b.unknown = Frame
	_, e := c.Execute(context.Background(), []byte("return42"), 1000)
	if !errors.Is(e, ErrUnknown) {
		t.Fatal(e)
	}
	m := b.writes[0]
	b.box.Phase = "validating"
	b.box.Ready = false
	b.box.BusinessReady = false
	b.box.Validation = &ValidationProgress{RequestID: m.Header.RequestID, RequestSHA256: m.Header.RequestSHA256, TotalBytes: uint32(len(m.Payload))}
	c.Now = func() time.Time { return time.UnixMilli(int64(m.Header.CreatedUTCMillis) + 70000) }
	if _, e = c.Step(context.Background()); !errors.Is(e, ErrPending) {
		t.Fatal("bounded validation consumed execution budget", e)
	}
	c.Now = func() time.Time { return time.UnixMilli(int64(m.Header.CreatedUTCMillis) + 600001) }
	if _, e = c.Step(context.Background()); !errors.Is(e, ErrBudget) {
		t.Fatal("transfer deadline not enforced", e)
	}
	if b.executions != 0 || len(b.writes) != 1 {
		t.Fatal("validation timeout inferred execution or replayed")
	}
}
