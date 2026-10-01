package command

import (
	"bufio"
	"context"
	"crypto/rand"
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"github.com/follenfang/lycheedev/internal/codebase"
	"github.com/follenfang/lycheedev/internal/codebase/environment"
	"github.com/follenfang/lycheedev/internal/luals"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
	"io"
	"net"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"sync"
	"time"
)

const brokerRequestBytes = 64 << 10

type brokerRequest struct {
	Wire                string               `json:"wire"`
	Identity            string               `json:"identity"`
	Action              string               `json:"action"`
	Deadline            time.Time            `json:"deadline"`
	Pin                 selection.SourcePin  `json:"pin"`
	RecordsHash         string               `json:"recordsHash"`
	EnvironmentSnapshot string               `json:"environmentSnapshot"`
	Environment         environment.Manifest `json:"environment"`
	Batches             [][]luals.Query      `json:"batches"`
}
type brokerHello struct {
	Wire       string `json:"wire"`
	Identity   string `json:"identity"`
	PID        uint32 `json:"pid"`
	Started    uint64 `json:"started"`
	Executable string `json:"executable"`
	Release    string `json:"release"`
}
type brokerResponse struct {
	Analysis  luals.Analysis `json:"analysis"`
	Error     string         `json:"error,omitempty"`
	State     string         `json:"state"`
	WorkerPID int            `json:"workerPid"`
	Batches   int            `json:"batches"`
	Reused    bool           `json:"reused"`
}

func brokerIdentity(home, release string) (string, error) {
	exe, err := os.Executable()
	if err != nil {
		return "", err
	}
	_, exeHash, err := luals.BrokerProcessIdentity(uint32(os.Getpid()))
	if err != nil {
		return "", err
	}
	file, err := os.Open(filepath.Join(release, "release.json"))
	if err != nil {
		return "", err
	}
	defer file.Close()
	raw, err := io.ReadAll(io.LimitReader(file, (1<<20)+1))
	if err != nil || len(raw) > 1<<20 {
		return "", luals.ErrUnavailable
	}
	digest := sha256.Sum256(raw)
	key := sha256.Sum256([]byte(luals.BrokerWire + "\x00" + strings.ToLower(home) + "\x00" + strings.ToLower(exe) + "\x00" + exeHash + "\x00" + hex.EncodeToString(digest[:])))
	return hex.EncodeToString(key[:]), nil
}

func writeBrokerFrame(writer io.Writer, value any, maximum int) error {
	raw, err := json.Marshal(value)
	if err != nil {
		return err
	}
	if len(raw) > maximum {
		return luals.ErrBudget
	}
	frame := make([]byte, 4+len(raw))
	binary.LittleEndian.PutUint32(frame, uint32(len(raw)))
	copy(frame[4:], raw)
	_, err = writer.Write(frame)
	return err
}
func readBrokerFrame(reader io.Reader, value any, maximum int) error {
	var header [4]byte
	if _, err := io.ReadFull(reader, header[:]); err != nil {
		return err
	}
	size := binary.LittleEndian.Uint32(header[:])
	if size == 0 || size > uint32(maximum) {
		return luals.ErrBudget
	}
	raw := make([]byte, size)
	if _, err := io.ReadFull(reader, raw); err != nil {
		return err
	}
	decoder := json.NewDecoder(strings.NewReader(string(raw)))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(value); err != nil {
		return err
	}
	if err := decoder.Decode(new(any)); err != io.EOF {
		return luals.ErrFailed
	}
	return nil
}

func connectSourceBroker(ctx context.Context, home, release string, start bool) (net.Conn, string, error) {
	var err error
	home, err = filepath.EvalSymlinks(home)
	if err != nil {
		return nil, "", err
	}
	home, err = filepath.Abs(home)
	if err != nil {
		return nil, "", err
	}
	release, err = filepath.EvalSymlinks(release)
	if err != nil {
		return nil, "", err
	}
	release, err = filepath.Abs(release)
	if err != nil {
		return nil, "", err
	}
	endpoint, _, _, err := luals.BrokerScope(home)
	if err != nil {
		return nil, "", err
	}
	identity, err := brokerIdentity(home, release)
	if err != nil {
		return nil, "", err
	}
	connect := func() (net.Conn, error) { return luals.DialBrokerPipe(ctx, endpoint) }
	conn, err := connect()
	if err != nil && os.IsNotExist(err) && start {
		launch, lockErr := vault.AcquireLease(ctx, filepath.Join(home, "locks"), "source:v1:broker-launch")
		if lockErr != nil {
			return nil, "", lockErr
		}
		defer launch.Close()
		conn, err = connect()
		if err != nil && os.IsNotExist(err) {
			var token [32]byte
			if _, err = rand.Read(token[:]); err != nil {
				return nil, "", err
			}
			started, _, e := luals.BrokerProcessIdentity(uint32(os.Getpid()))
			if e != nil {
				return nil, "", e
			}
			exe, e := os.Executable()
			if e != nil {
				return nil, "", e
			}
			exe, e = filepath.Abs(exe)
			if e != nil {
				return nil, "", e
			}
			if e = luals.StartBroker(exe, []string{"--internal-luals-broker", home, release, strconv.Itoa(os.Getpid()), strconv.FormatUint(started, 10), hex.EncodeToString(token[:])}); e != nil {
				return nil, "", e
			}
			startup := time.NewTimer(5 * time.Second)
			defer startup.Stop()
			for {
				conn, err = connect()
				if err == nil {
					break
				}
				if !os.IsNotExist(err) {
					return nil, "", err
				}
				delay := time.NewTimer(25 * time.Millisecond)
				select {
				case <-ctx.Done():
					delay.Stop()
					return nil, "", ctx.Err()
				case <-startup.C:
					delay.Stop()
					return nil, "", fmt.Errorf("%w: broker startup unavailable", luals.ErrUnavailable)
				case <-delay.C:
				}
			}
		}
	}
	if err != nil {
		return nil, "", err
	}
	stopHello := context.AfterFunc(ctx, func() { conn.Close() })
	defer stopHello()
	deadline, ok := ctx.Deadline()
	if !ok {
		deadline = time.Now().Add(90 * time.Second)
	}
	_ = conn.SetDeadline(deadline)
	var hello brokerHello
	if err = readBrokerFrame(conn, &hello, 4096); err == nil {
		if hello.Wire != luals.BrokerWire || hello.Identity != identity {
			err = fmt.Errorf("%w: broker version/context mismatch", luals.ErrUnavailable)
		} else {
			_, hash, e := luals.BrokerProcessIdentity(uint32(os.Getpid()))
			if e != nil {
				err = e
			} else {
				err = luals.VerifyBrokerPeer(conn, hello.PID, hello.Started, hash)
			}
		}
	}
	if err != nil {
		conn.Close()
		return nil, "", err
	}
	if ctx.Err() != nil {
		conn.Close()
		return nil, "", ctx.Err()
	}
	return conn, identity, nil
}

func sourceBrokerAnalyzer(home, release, environmentSnapshot string) codebase.SemanticAnalyzer {
	return func(ctx context.Context, pin selection.SourcePin, recordsHash string, env environment.Manifest, queries []luals.Query) (luals.Analysis, error) {
		conn, identity, err := connectSourceBroker(ctx, home, release, true)
		if err != nil {
			return luals.Analysis{}, err
		}
		defer conn.Close()
		stop := context.AfterFunc(ctx, func() { conn.Close() })
		defer stop()
		deadline, _ := ctx.Deadline()
		req := brokerRequest{Wire: luals.BrokerWire, Identity: identity, Action: "analyze", Deadline: deadline, Pin: pin, RecordsHash: recordsHash, EnvironmentSnapshot: environmentSnapshot, Environment: env, Batches: luals.SplitQueries(queries)}
		if err = writeBrokerFrame(conn, req, brokerRequestBytes); err != nil {
			return luals.Analysis{}, err
		}
		var response brokerResponse
		if err = readBrokerFrame(conn, &response, luals.MaxReportBytes); err != nil {
			if ctx.Err() != nil {
				return response.Analysis, ctx.Err()
			}
			return response.Analysis, fmt.Errorf("%w: broker response: %v", luals.ErrFailed, err)
		}
		if response.Error != "" {
			return response.Analysis, fmt.Errorf("%w: %s", luals.ErrFailed, response.Error)
		}
		return response.Analysis, nil
	}
}

// SourceBrokerMain is an internal process entry, outside the user command
// directory. A launch ticket and the live launching process must both match.
func SourceBrokerMain(ctx context.Context, args []string) int {
	if len(args) != 6 || args[0] != "--internal-luals-broker" || args[5] == "" || len(args[5]) != 64 || os.Getenv("LYCHEEDEV_BROKER_START") != args[5] {
		return 2
	}
	_ = os.Unsetenv("LYCHEEDEV_BROKER_START")
	parent, err := strconv.ParseUint(args[3], 10, 32)
	if err != nil {
		return 2
	}
	started, err := strconv.ParseUint(args[4], 10, 64)
	if err != nil {
		return 2
	}
	got, parentHash, err := luals.BrokerProcessIdentity(uint32(parent))
	if err != nil || got != started {
		return 3
	}
	_, selfHash, err := luals.BrokerProcessIdentity(uint32(os.Getpid()))
	if err != nil || selfHash != parentHash {
		return 3
	}
	home, err := filepath.EvalSymlinks(args[1])
	if err != nil {
		return 3
	}
	home, err = filepath.Abs(home)
	if err != nil {
		return 3
	}
	release, err := filepath.EvalSymlinks(args[2])
	if err != nil {
		return 3
	}
	release, err = filepath.Abs(release)
	if err != nil {
		return 3
	}
	closeCustody, err := luals.OwnBrokerJob()
	if err != nil {
		return 3
	}
	defer closeCustody()
	runtime, err := openLuaLS(ctx, release)
	if err != nil {
		return 3
	}
	endpoint, sddl, scope, err := luals.BrokerScope(home)
	if err != nil {
		return 3
	}
	// One opt-in worker per logon across homes prevents each home from reserving
	// the entire machine budget. Competing homes fail closed, never kill an owner.
	aggregate, err := vault.TryAcquireLease(ctx, scope, "source-broker-aggregate.v1")
	if err != nil {
		return 3
	}
	defer aggregate.Close()
	owner, err := vault.TryAcquireLease(ctx, filepath.Join(home, "locks"), "source:v1:broker-owner")
	if err != nil {
		return 3
	}
	defer owner.Close()
	identity, err := brokerIdentity(home, release)
	if err != nil {
		return 3
	}
	listener, err := luals.ListenBrokerPipe(endpoint, sddl)
	if err != nil {
		return 3
	}
	defer listener.Close()
	store, err := vault.OpenStore(home)
	if err != nil {
		return 3
	}
	artifacts := filepath.Join(home, "tmp", "luals-session")
	if err = prepareBrokerArtifacts(home, artifacts); err != nil {
		return 3
	}
	browser := codebase.OpenBrowser(store)
	life, cancel := context.WithTimeout(ctx, 10*time.Minute)
	defer cancel()
	context.AfterFunc(life, func() { listener.Close() })
	stopReleaseWatch, err := luals.WatchIdentityDirectory(release, cancel)
	if err != nil {
		return 3
	}
	defer stopReleaseWatch()
	helloStarted, _, err := luals.BrokerProcessIdentity(uint32(os.Getpid()))
	if err != nil {
		return 3
	}
	hello := brokerHello{Wire: luals.BrokerWire, Identity: identity, PID: uint32(os.Getpid()), Started: helloStarted, Executable: selfHash, Release: release}
	queue := newSourceBrokerQueue()
	slots := make(chan struct{}, 17)
	var handlers sync.WaitGroup
	defer handlers.Wait()
	workerDone := make(chan struct{})
	defer func() { cancel(); <-workerDone }()
	go func() {
		defer close(workerDone)
		defer cancel()
		var worker *luals.WorkspaceSession
		var lease *codebase.WorktreeLease
		key := ""
		batches := 0
		retire := func() {
			if worker != nil {
				worker.Close()
				worker = nil
			}
			if lease != nil {
				lease.Close()
				lease = nil
			}
			key = ""
			batches = 0
		}
		defer retire()
		idle := time.NewTimer(60 * time.Second)
		defer idle.Stop()
		for {
			select {
			case <-life.Done():
				return
			case <-idle.C:
				return
			case <-queue.wake:
				task := queue.next()
				if task == nil {
					continue
				}
				if !idle.Stop() {
					select {
					case <-idle.C:
					default:
					}
				}
				if worker != nil && (!worker.Usable() || worker.BatchCount()+len(task.req.Batches) > 256) {
					retire()
				}
				response := brokerResponse{State: "partial"}
				if task.ctx.Err() != nil {
					response.Error = task.ctx.Err().Error()
					task.reply <- response
					idle.Reset(60 * time.Second)
					continue
				}
				if task.req.Action == "retire" {
					retire()
					response.State = "closed"
					task.reply <- response
					select {
					case <-task.done:
					case <-life.Done():
					}
					return
				}
				if task.req.Action == "status" {
					response.State = "idle"
					if worker != nil {
						response.State = "ready"
						response.WorkerPID = worker.PID()
					}
					response.Batches = batches
					task.reply <- response
					idle.Reset(60 * time.Second)
					continue
				}
				if task.req.Action != "analyze" {
					response.Error = "unsupported broker action"
					task.reply <- response
					idle.Reset(60 * time.Second)
					continue
				}
				runtime, err = openLuaLS(task.ctx, release)
				if err == nil {
					admission := task.req.Pin.Repository + "\x00" + task.req.Pin.Product + "\x00" + task.req.Pin.ExactCommit
					if key != "" && !strings.HasPrefix(key, admission+"\x00") {
						retire()
					}
					var verified codebase.SemanticWorkspace
					verified, err = browser.VerifySemanticWorkspace(task.ctx, task.req.Pin, task.req.EnvironmentSnapshot, task.req.RecordsHash, task.req.Environment, lease)
					if err == nil {
						next := admission + "\x00" + verified.Identity
						if worker != nil && key != next {
							retire()
							verified, err = browser.VerifySemanticWorkspace(task.ctx, task.req.Pin, task.req.EnvironmentSnapshot, task.req.RecordsHash, task.req.Environment, nil)
						}
						if err == nil {
							lease = verified.Lease
							response.Reused = worker != nil
							key = next
							if worker == nil {
								worker, err = runtime.OpenBrokerWorkspaceSession(task.ctx, lease.Path(), verified.Definitions, artifacts)
							}
							if err == nil {
								response.Analysis, err = worker.AnalyzeBatches(task.ctx, task.req.Batches)
								batches += len(task.req.Batches)
								response.Batches = batches
								response.WorkerPID = worker.PID()
								response.State = response.Analysis.State
							}
						}
					}
				}
				if err != nil {
					response.Error = err.Error()
					retire()
				}
				if batches >= 256 {
					retire()
				}
				task.reply <- response
				idle.Reset(60 * time.Second)
			}
		}
	}()
	for {
		conn, e := listener.Accept()
		if e != nil {
			break
		}
		select {
		case slots <- struct{}{}:
		default:
			conn.Close()
			continue
		}
		handlers.Add(1)
		go func() {
			defer handlers.Done()
			defer func() { <-slots }()
			defer conn.Close()
			_ = conn.SetDeadline(time.Now().Add(5 * time.Second))
			if writeBrokerFrame(conn, hello, 4096) != nil {
				return
			}
			var request brokerRequest
			if readBrokerFrame(conn, &request, brokerRequestBytes) != nil {
				return
			}
			if request.Wire != luals.BrokerWire || request.Identity != identity || len(request.Batches) > 2 || request.Deadline.IsZero() || request.Deadline.After(time.Now().Add(90*time.Second)) {
				return
			}
			for _, batch := range request.Batches {
				if len(batch) > 128 {
					return
				}
			}
			caller, stop := context.WithDeadline(life, request.Deadline)
			defer stop()
			_ = conn.SetDeadline(request.Deadline)
			go func() { var byte [1]byte; _, _ = bufio.NewReader(conn).Read(byte[:]); stop() }()
			reply := make(chan brokerResponse, 1)
			written := make(chan struct{})
			defer close(written)
			task := &sourceBrokerTask{ctx: caller, req: request, reply: reply, done: written}
			if !queue.submit(task) {
				_ = writeBrokerFrame(conn, brokerResponse{Error: "bounded broker queue full", State: "partial"}, luals.MaxReportBytes)
				return
			}
			defer queue.remove(task)
			select {
			case response := <-reply:
				_ = writeBrokerFrame(conn, response, luals.MaxReportBytes)
			case <-caller.Done():
			}
		}()
	}
	cancel()
	return 0
}

// retireSourceBroker is read-only when no endpoint exists. It never launches a
// broker, kills a PID, or releases a lease belonging to an unknown owner.
func retireSourceBroker(ctx context.Context, home, release string) error {
	conn, identity, err := connectSourceBroker(ctx, home, release, false)
	if err != nil {
		if os.IsNotExist(err) {
			return nil
		}
		return err
	}
	defer conn.Close()
	deadline, _ := ctx.Deadline()
	if deadline.IsZero() {
		deadline = time.Now().Add(5 * time.Second)
	}
	if err = writeBrokerFrame(conn, brokerRequest{Wire: luals.BrokerWire, Identity: identity, Action: "retire", Deadline: deadline}, brokerRequestBytes); err != nil {
		return err
	}
	var response brokerResponse
	if err = readBrokerFrame(conn, &response, luals.MaxReportBytes); err != nil {
		return err
	}
	if response.State != "closed" {
		return errors.New("broker retirement not confirmed")
	}
	return nil
}

func runSourceSession(ctx context.Context, route string, opts Options, response *Envelope) (int, error) {
	bounded, cancel := context.WithTimeout(ctx, 5*time.Second)
	defer cancel()
	home, err := workspaceRoot(opts.home)
	if err != nil {
		return 0, err
	}
	runtime, err := openLuaLS(bounded, opts.release)
	if err != nil {
		return 0, err
	}
	if route == "source session close" {
		err = retireSourceBroker(bounded, home, runtime.ReleaseRoot())
		response.Result = map[string]any{"state": "closed"}
		return 0, err
	}
	conn, identity, err := connectSourceBroker(bounded, home, runtime.ReleaseRoot(), false)
	if err != nil {
		if os.IsNotExist(err) {
			response.Result = map[string]any{"state": "closed"}
			return 0, nil
		}
		return 0, err
	}
	defer conn.Close()
	deadline, _ := bounded.Deadline()
	if err = writeBrokerFrame(conn, brokerRequest{Wire: luals.BrokerWire, Identity: identity, Action: "status", Deadline: deadline}, brokerRequestBytes); err != nil {
		return 0, err
	}
	var result brokerResponse
	err = readBrokerFrame(conn, &result, luals.MaxReportBytes)
	response.Result = map[string]any{"state": result.State, "workerPid": result.WorkerPID, "batches": result.Batches}
	return 0, err
}

// The owner/aggregate leases make this a single managed temporary namespace.
// Stale worker artifacts are disposable; source facts and checkouts are outside
// it. Reject links/foreign contents before any recursive removal.
func prepareBrokerArtifacts(home, dir string) error {
	homeAbs, err := filepath.Abs(home)
	if err != nil {
		return err
	}
	dirAbs, err := filepath.Abs(dir)
	if err != nil {
		return err
	}
	rel, err := filepath.Rel(homeAbs, dirAbs)
	if err != nil || rel != filepath.Join("tmp", "luals-session") {
		return luals.ErrUnavailable
	}
	if info, err := os.Lstat(dir); err == nil {
		if !info.IsDir() || info.Mode()&os.ModeSymlink != 0 {
			return luals.ErrUnavailable
		}
	} else if !os.IsNotExist(err) {
		return err
	}
	if err = os.MkdirAll(dir, 0700); err != nil {
		return err
	}
	resolved, err := filepath.EvalSymlinks(dir)
	if err != nil || !strings.EqualFold(resolved, dirAbs) {
		return luals.ErrUnavailable
	}
	marker := filepath.Join(dir, ".lycheedev-luals-session-v1")
	entries, err := os.ReadDir(dir)
	if err != nil {
		return err
	}
	raw, markerErr := os.ReadFile(marker)
	if markerErr != nil {
		if !os.IsNotExist(markerErr) || len(entries) != 0 {
			return luals.ErrUnavailable
		}
		return os.WriteFile(marker, []byte(luals.BrokerWire), 0600)
	}
	if string(raw) != luals.BrokerWire || len(entries) > 129 {
		return luals.ErrBudget
	}
	for _, entry := range entries {
		if entry.Name() == filepath.Base(marker) {
			continue
		}
		if !entry.IsDir() || entry.Type()&os.ModeSymlink != 0 || !strings.HasPrefix(entry.Name(), "lycheedev-luals-lsp-") {
			return luals.ErrUnavailable
		}
		child := filepath.Join(dir, entry.Name())
		nodes := 0
		if err = filepath.WalkDir(child, func(path string, item os.DirEntry, walkErr error) error {
			if walkErr != nil {
				return walkErr
			}
			nodes++
			if nodes > 10000 {
				return luals.ErrBudget
			}
			if item.Type()&os.ModeSymlink != 0 {
				return luals.ErrUnavailable
			}
			return nil
		}); err != nil {
			return err
		}
		if err = os.RemoveAll(child); err != nil {
			return err
		}
	}
	return nil
}

func retireSourceBrokerIfPresent(ctx context.Context, home string) error {
	bounded, cancel := context.WithTimeout(ctx, 5*time.Second)
	defer cancel()
	endpoint, _, _, err := luals.BrokerScope(home)
	if err != nil {
		return err
	}
	conn, err := luals.DialBrokerPipe(bounded, endpoint)
	if err != nil {
		if os.IsNotExist(err) {
			return nil
		}
		return err
	}
	defer conn.Close()
	stop := context.AfterFunc(bounded, func() { conn.Close() })
	defer stop()
	deadline, _ := bounded.Deadline()
	_ = conn.SetDeadline(deadline)
	var hello brokerHello
	if err = readBrokerFrame(conn, &hello, 4096); err != nil {
		return err
	}
	_, selfHash, err := luals.BrokerProcessIdentity(uint32(os.Getpid()))
	if err != nil {
		return err
	}
	if err = luals.VerifyBrokerPeer(conn, hello.PID, hello.Started, selfHash); err != nil {
		return err
	}
	if hello.Wire != luals.BrokerWire {
		return luals.ErrUnavailable
	}
	runtime, err := openLuaLS(bounded, hello.Release)
	if err != nil {
		return err
	}
	identity, err := brokerIdentity(home, runtime.ReleaseRoot())
	if err != nil || identity != hello.Identity {
		return luals.ErrUnavailable
	}
	if err = writeBrokerFrame(conn, brokerRequest{Wire: luals.BrokerWire, Identity: identity, Action: "retire", Deadline: deadline}, brokerRequestBytes); err != nil {
		return err
	}
	var response brokerResponse
	if err = readBrokerFrame(conn, &response, luals.MaxReportBytes); err != nil {
		return err
	}
	if response.State != "closed" {
		return luals.ErrFailed
	}
	return nil
}

type sourceBrokerTask struct {
	ctx   context.Context
	req   brokerRequest
	reply chan brokerResponse
	done  <-chan struct{}
}
type sourceBrokerQueue struct {
	mu      sync.Mutex
	pending []*sourceBrokerTask
	wake    chan struct{}
}

func newSourceBrokerQueue() *sourceBrokerQueue {
	return &sourceBrokerQueue{wake: make(chan struct{}, 1)}
}
func (q *sourceBrokerQueue) submit(task *sourceBrokerTask) bool {
	q.mu.Lock()
	defer q.mu.Unlock()
	if len(q.pending) >= 16 {
		return false
	}
	q.pending = append(q.pending, task)
	select {
	case q.wake <- struct{}{}:
	default:
	}
	return true
}
func (q *sourceBrokerQueue) remove(task *sourceBrokerTask) {
	q.mu.Lock()
	defer q.mu.Unlock()
	for i, pending := range q.pending {
		if pending == task {
			copy(q.pending[i:], q.pending[i+1:])
			q.pending[len(q.pending)-1] = nil
			q.pending = q.pending[:len(q.pending)-1]
			return
		}
	}
}
func (q *sourceBrokerQueue) next() *sourceBrokerTask {
	q.mu.Lock()
	defer q.mu.Unlock()
	if len(q.pending) == 0 {
		return nil
	}
	task := q.pending[0]
	copy(q.pending, q.pending[1:])
	q.pending[len(q.pending)-1] = nil
	q.pending = q.pending[:len(q.pending)-1]
	if len(q.pending) > 0 {
		select {
		case q.wake <- struct{}{}:
		default:
		}
	}
	return task
}
