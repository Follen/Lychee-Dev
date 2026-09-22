package live

import (
	"context"
	"errors"
	"fmt"
	"image"
	"io"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/buildinfo"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/records"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
)

// fakeDisplay models one window's persistent live frame stream: every queued
// frame is delivered exactly once to the next reader, and frames queued before
// a capture opens are still delivered, like a lingering on-screen receipt.
type fakeDisplay struct {
	mu       sync.Mutex
	cond     *sync.Cond
	queue    []*desktop.CapturedFrame
	next     int
	finished bool
}

func newFakeDisplay() *fakeDisplay {
	d := &fakeDisplay{}
	d.cond = sync.NewCond(&d.mu)
	return d
}

func (d *fakeDisplay) push(frames ...*desktop.CapturedFrame) {
	d.mu.Lock()
	d.queue = append(d.queue, frames...)
	d.finished = true
	d.cond.Broadcast()
	d.mu.Unlock()
}

func (d *fakeDisplay) open() *fakeFeed { return &fakeFeed{display: d} }

type fakeFeed struct {
	display *fakeDisplay
	closed  bool
}

func (f *fakeFeed) Next(ctx context.Context) (*desktop.CapturedFrame, error) {
	done := make(chan struct{})
	defer close(done)
	go func() {
		select {
		case <-ctx.Done():
			f.display.mu.Lock()
			f.display.cond.Broadcast()
			f.display.mu.Unlock()
		case <-done:
		}
	}()
	f.display.mu.Lock()
	defer f.display.mu.Unlock()
	for f.display.next >= len(f.display.queue) && !f.display.finished {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		f.display.cond.Wait()
	}
	if f.display.next >= len(f.display.queue) {
		return nil, io.EOF
	}
	frame := f.display.queue[f.display.next]
	f.display.next++
	return frame, nil
}

func (f *fakeFeed) Close() { f.closed = true }

// fakeIO replaces the native seam. Responders echo the probe nonce like the
// real addon, so nonce correlation is actually exercised through QR frames.
type fakeIO struct {
	t          *testing.T
	windows    []desktop.WindowIdentity
	respond    map[uint64]func(command string) []*desktop.CapturedFrame
	sendErr    map[uint64]error
	confirmErr map[uint64]error
	owners     map[string]journal.WindowOwner
	sent       []string
	displays   map[uint64]*fakeDisplay
	ticks      map[uint64]int64
}

func newFakeIO(t *testing.T) *fakeIO {
	return &fakeIO{t: t,
		respond:    map[uint64]func(string) []*desktop.CapturedFrame{},
		sendErr:    map[uint64]error{},
		confirmErr: map[uint64]error{},
		owners:     map[string]journal.WindowOwner{},
		displays:   map[uint64]*fakeDisplay{},
		ticks:      map[uint64]int64{},
	}
}

func (f *fakeIO) display(handle uint64) *fakeDisplay {
	display, ok := f.displays[handle]
	if !ok {
		display = newFakeDisplay()
		f.displays[handle] = display
	}
	return display
}

func (f *fakeIO) frame(handle uint64, signal bridge.Signal) *desktop.CapturedFrame {
	frame := makeAckFrame(f.t, signal)
	f.ticks[handle]++
	frame.SystemTicks = f.ticks[handle]
	return frame
}

func (f *fakeIO) commands() []string {
	return append([]string{}, f.sent...)
}

func (f *fakeIO) sentKind(kind string) []string {
	matched := []string{}
	for _, command := range f.sent {
		if strings.Contains(command, kind) {
			matched = append(matched, command)
		}
	}
	return matched
}

func (f *fakeIO) io() *liveIO {
	return &liveIO{
		list: func(context.Context) ([]desktop.WindowIdentity, error) { return f.windows, nil },
		inspect: func(ctx context.Context, directory string) (selection.ClientInstallation, error) {
			return records.InspectClientInstallation(ctx, directory)
		},
		capture: func(_ context.Context, window desktop.WindowIdentity, _ image.Rectangle) (sessionFrames, error) {
			return f.display(window.Handle).open(), nil
		},
		send: func(_ context.Context, window desktop.WindowIdentity, command string) (desktop.InputReceipt, error) {
			f.sent = append(f.sent, fmt.Sprintf("%d:%s", window.Handle, command))
			if err := f.sendErr[window.Handle]; err != nil {
				return desktop.InputReceipt{}, err
			}
			respond := f.respond[window.Handle]
			if respond == nil {
				return desktop.InputReceipt{}, errors.New("fixture.no_response")
			}
			f.display(window.Handle).push(respond(command)...)
			return desktop.InputReceipt{MessagesQueued: 2, SubmissionComplete: true}, nil
		},
		confirm: func(_ context.Context, target ClientWindow) error {
			return f.confirmErr[target.Window.Handle]
		},
		peek: func(_ context.Context, _, resource string) (journal.WindowOwner, bool, error) {
			if owner, ok := f.owners[resource]; ok {
				return owner, true, nil
			}
			return journal.WindowOwner{}, false, nil
		},
		wait:    250 * time.Millisecond,
		refresh: 250 * time.Millisecond,
	}
}

// identityReceipt builds one identity marker exactly as the game verb emits it.
func identityReceipt(nonce string, mutate func(*bridge.Signal)) bridge.Signal {
	signal := bridge.Signal{Schema: "lycheedev.signal.v1", Release: buildinfo.Version, Kind: "identity",
		ProbeNonce: nonce, ActorState: "ok", Character: "Paladin", Realm: "Realm", GUID: "Player-1-123",
		Product: "retail", Build: "12.1.0.69875", InputReady: true}
	if mutate != nil {
		mutate(&signal)
	}
	return signal
}

func readyReceipt(mutate func(*bridge.Signal)) bridge.Signal {
	signal := bridge.Signal{Schema: "lycheedev.signal.v1", Release: buildinfo.Version, Kind: "ready",
		SessionNonce: strings.Repeat("a", 32), Character: "Paladin", Realm: "Realm", GUID: "Player-1-123",
		Product: "retail", Build: "12.1.0.69875", Sequence: 1}
	if mutate != nil {
		mutate(&signal)
	}
	return signal
}

// nonceFromCommand echoes the trigger nonce exactly like the game machine verb.
func nonceFromCommand(command string) string {
	const prefix = "/dev bridge identify "
	if !strings.HasPrefix(command, prefix) {
		return ""
	}
	return strings.TrimPrefix(command, prefix)
}

// identityResponder returns the standard first-display + focus-refresh pair.
// identityResponder returns the standard first-display + focus-refresh pair.
func identityResponder(f *fakeIO, handle uint64, mutate func(*bridge.Signal)) func(string) []*desktop.CapturedFrame {
	return func(command string) []*desktop.CapturedFrame {
		nonce := nonceFromCommand(command)
		unready := identityReceipt(nonce, func(signal *bridge.Signal) {
			signal.InputReady, signal.InputReason = false, inputKeyboardFocus
			if mutate != nil {
				mutate(signal)
			}
		})
		refreshed := identityReceipt(nonce, mutate)
		return []*desktop.CapturedFrame{f.frame(handle, unready), f.frame(handle, refreshed)}
	}
}

// makeTestClient creates one supported client folder with a fake game
// executable the native identity checks can resolve.
func makeTestClient(t *testing.T, gameRoot, folder, flavor, version string) string {
	t.Helper()
	client := filepath.Join(gameRoot, folder)
	if err := os.MkdirAll(client, 0700); err != nil {
		t.Fatal(err)
	}
	writeTestFile(t, filepath.Join(client, ".flavor.info"), "Product Flavor!STRING:0\n"+flavor+"\n")
	writeTestFile(t, filepath.Join(client, "version.txt"), version+"\n")
	writeTestFile(t, filepath.Join(client, "Wow.exe"), "fixture executable\n")
	return client
}

func writeTestFile(t *testing.T, path, content string) {
	t.Helper()
	if err := os.WriteFile(path, []byte(content), 0600); err != nil {
		t.Fatal(err)
	}
}

func testWindow(handle uint64, client string) desktop.WindowIdentity {
	return desktop.WindowIdentity{Handle: handle, ProcessID: uint32(1000 + handle), ProcessStartedAt: 55, Executable: filepath.Join(client, "Wow.exe"), Class: "OsWindow", Title: "World of Warcraft"}
}

func testPin(t *testing.T, root string) string {
	t.Helper()
	ctx := context.Background()
	store, err := vault.Initialize(ctx, root)
	if err != nil {
		t.Fatal(err)
	}
	metadata, err := store.OpenMetadata(ctx)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { metadata.Close() })
	pin, err := selection.OpenPinner(metadata).PinSelection(ctx, selection.SelectionSpec{Source: &selection.SourcePin{Repository: "fixture", Product: "retail", ExactCommit: strings.Repeat("a", 40), ParserRevision: "1"}})
	if err != nil {
		t.Fatal(err)
	}
	return pin.ID
}

func TestDiscoveryIdentifiesWindowsWithPerWindowIsolation(t *testing.T) {
	ctx := context.Background()
	gameRoot := t.TempDir()
	client := makeTestClient(t, gameRoot, "_retail_", "wow", "12.1.0.69875")
	fake := newFakeIO(t)
	fake.windows = []desktop.WindowIdentity{testWindow(1, client), testWindow(2, client), testWindow(3, client), testWindow(4, client)}
	fake.respond[1] = identityResponder(fake, 1, nil)
	fake.respond[2] = identityResponder(fake, 2, func(signal *bridge.Signal) {
		signal.ActorState, signal.Character, signal.Realm, signal.GUID = "no_actor", "", "", ""
		signal.InputReady, signal.InputReason = false, "input_not_logged_in"
	})
	fake.respond[3] = func(string) []*desktop.CapturedFrame { return nil } // addon missing / input lost
	fake.respond[4] = func(command string) []*desktop.CapturedFrame {
		black := image.NewNRGBA(image.Rect(0, 0, 320, 320))
		frame := &desktop.CapturedFrame{NRGBA: black, SystemTicks: 1, ObservedAt: time.Now()}
		return []*desktop.CapturedFrame{frame}
	}
	report, err := discoverCandidates(ctx, filepath.Join(gameRoot, "home"), DiscoveryRequest{}, fake.io())
	if err != nil {
		t.Fatal(err)
	}
	if len(report.Candidates) != 4 {
		t.Fatalf("candidates dropped: %+v", report.Candidates)
	}
	states := []string{}
	for _, candidate := range report.Candidates {
		states = append(states, candidate.State)
	}
	want := []string{CandidateIdentified, CandidateNoActor, CandidateUnreadable, CandidateUnreadable}
	for i := range want {
		if states[i] != want[i] {
			t.Fatalf("states %v, want %v", states, want)
		}
	}
	identified := report.Candidates[0]
	if identified.Character != "Paladin" || identified.Realm != "Realm" || identified.GUID != "Player-1-123" || identified.Client.Product != "retail" {
		t.Fatalf("identified candidate: %+v", identified)
	}
	// Discovery reports the display-time input fact without gating on it.
	if identified.InputReady || identified.NotReadyReason != inputKeyboardFocus {
		t.Fatalf("input fact: %+v", identified)
	}
	noActor := report.Candidates[1]
	if noActor.Character != "" || noActor.Realm != "" || noActor.NotReadyReason != "input_not_logged_in" {
		t.Fatalf("no_actor candidate: %+v", noActor)
	}
	if report.Candidates[2].Capture != captureNoFrame || report.Candidates[3].Capture != captureBlackFrame {
		t.Fatalf("capture hints: %+v %+v", report.Candidates[2], report.Candidates[3])
	}
	for _, candidate := range report.Candidates {
		if candidate.Character != "" && candidate.State != CandidateIdentified {
			t.Fatal("fabricated identity:", candidate)
		}
	}
}

func TestDiscoverySkipsBusyWindowsWithoutTyping(t *testing.T) {
	ctx := context.Background()
	gameRoot := t.TempDir()
	client := makeTestClient(t, gameRoot, "_retail_", "wow", "12.1.0.69875")
	fake := newFakeIO(t)
	fake.windows = []desktop.WindowIdentity{testWindow(1, client), testWindow(2, client)}
	fake.respond[1] = identityResponder(fake, 1, nil)
	fake.owners[windowHandleResource(fake.windows[1])] = journal.WindowOwner{
		Schema: "lycheedev.window-owner.v1", WorkspaceID: strings.Repeat("b", 32),
		Resource: windowHandleResource(fake.windows[1]), OperationID: "OP-" + strings.Repeat("c", 32),
	}
	report, err := discoverCandidates(ctx, filepath.Join(gameRoot, "home"), DiscoveryRequest{}, fake.io())
	if err != nil {
		t.Fatal(err)
	}
	if len(report.Candidates) != 2 {
		t.Fatalf("candidates: %+v", report.Candidates)
	}
	busy := report.Candidates[1]
	if busy.State != CandidateBusy || busy.BusyOperation != "OP-"+strings.Repeat("c", 32) || !busy.ForeignOwner || busy.Character != "" {
		t.Fatalf("busy candidate: %+v", busy)
	}
	for _, command := range fake.commands() {
		if strings.HasPrefix(command, "2:") {
			t.Fatal("typed into a busy window:", command)
		}
	}
	if len(fake.commands()) != 1 {
		t.Fatalf("unexpected input: %v", fake.commands())
	}
}

func TestDiscoveryKeepsIdenticalCandidatesDistinct(t *testing.T) {
	ctx := context.Background()
	gameRoot := t.TempDir()
	client := makeTestClient(t, gameRoot, "_retail_", "wow", "12.1.0.69875")
	fake := newFakeIO(t)
	fake.windows = []desktop.WindowIdentity{testWindow(1, client), testWindow(2, client)}
	fake.respond[1] = identityResponder(fake, 1, nil)
	fake.respond[2] = identityResponder(fake, 2, nil)
	report, err := discoverCandidates(ctx, filepath.Join(gameRoot, "home"), DiscoveryRequest{}, fake.io())
	if err != nil {
		t.Fatal(err)
	}
	a, b := report.Candidates[0], report.Candidates[1]
	if a.Character != b.Character || a.Realm != b.Realm || a.GUID != b.GUID || a.Client != b.Client {
		t.Fatal("fixture candidates are not identical")
	}
	if a.Window.Handle == b.Window.Handle || a.Window.ProcessID == b.Window.ProcessID {
		t.Fatal("identical candidates collapsed into one entry")
	}
}

func TestDiscoveryRejectsStaleAndForeignReceipts(t *testing.T) {
	ctx := context.Background()
	gameRoot := t.TempDir()
	client := makeTestClient(t, gameRoot, "_retail_", "wow", "12.1.0.69875")
	for _, mode := range []string{"foreign-nonce", "wrong-build", "ready-kind"} {
		t.Run(mode, func(t *testing.T) {
			fake := newFakeIO(t)
			fake.windows = []desktop.WindowIdentity{testWindow(1, client)}
			fake.respond[1] = func(command string) []*desktop.CapturedFrame {
				nonce := nonceFromCommand(command)
				signal := identityReceipt(nonce, nil)
				switch mode {
				case "foreign-nonce":
					signal.ProbeNonce = strings.Repeat("f", 32)
				case "wrong-build":
					signal.Build = "12.1.0.69876"
				case "ready-kind":
					signal.Kind, signal.ProbeNonce, signal.ActorState = "ready", "", ""
					signal.SessionNonce, signal.Sequence, signal.InputReady = strings.Repeat("a", 32), 1, true
				}
				return []*desktop.CapturedFrame{fake.frame(1, signal)}
			}
			report, err := discoverCandidates(ctx, filepath.Join(gameRoot, "home"), DiscoveryRequest{}, fake.io())
			if err != nil {
				t.Fatal(err)
			}
			if report.Candidates[0].State != CandidateUnreadable {
				t.Fatalf("accepted %s: %+v", mode, report.Candidates[0])
			}
		})
	}
}

func TestDiscoveryScansOnlyKnownRootsAndProducts(t *testing.T) {
	ctx := context.Background()
	gameRoot := t.TempDir()
	retail := makeTestClient(t, gameRoot, "_retail_", "wow", "12.1.0.69875")
	makeTestClient(t, gameRoot, "_classic_", "wow_classic", "5.5.4.12345")
	makeTestClient(t, gameRoot, "_classic_beta_", "wow_beta", "12.1.0.69875") // unsupported product: excluded
	makeTestClient(t, gameRoot, "_ptr_", "wow", "12.1.0.69875")               // never scanned
	fake := newFakeIO(t)
	fake.windows = []desktop.WindowIdentity{testWindow(1, retail)}
	fake.respond[1] = identityResponder(fake, 1, nil)
	root := filepath.Join(gameRoot, "home")
	if err := os.MkdirAll(root, 0700); err != nil {
		t.Fatal(err)
	}
	report, err := discoverCandidates(ctx, root, DiscoveryRequest{Roots: []string{gameRoot, filepath.Join(gameRoot, "absent")}}, fake.io())
	if err != nil {
		t.Fatal(err)
	}
	if len(report.Installations) != 2 {
		t.Fatalf("installations: %+v", report.Installations)
	}
	states := map[string]string{}
	for _, installation := range report.Installations {
		states[filepath.Base(installation.Client.Directory)] = installation.State
	}
	if states["_retail_"] != InstallationRunning || states["_classic_"] != InstallationInstalled {
		t.Fatalf("installation states: %v", states)
	}
	empty, err := discoverCandidates(ctx, root, DiscoveryRequest{}, fake.io())
	if err != nil || len(empty.Installations) != 1 || empty.Installations[0].State != InstallationRunning {
		t.Fatalf("running installations still reported: %+v %v", empty.Installations, err)
	}
}
