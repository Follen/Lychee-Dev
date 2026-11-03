package publish

import (
	"bytes"
	"context"
	"crypto/subtle"
	"encoding/binary"
	"encoding/json"
	"errors"
	"io"
	"net"
	"net/url"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"sync"
	"time"

	"github.com/follenfang/lycheedev/internal/vault"
)

const ddSessionWire = "lycheedev.publish.dd-session.v1"
const ddWorkerFlag = "--publish-dd-session-worker"
const ddSessionLimit = 16 << 20

type ddSessionState struct {
	Schema   string         `json:"schema"`
	ID       string         `json:"id"`
	Nonce    string         `json:"nonce"`
	Pipe     string         `json:"pipe"`
	PID      int            `json:"pid"`
	Created  uint64         `json:"created"`
	Status   string         `json:"status"`
	Identity map[string]any `json:"identity,omitempty"`
}
type ddSessionOpen struct {
	Home, ID, Nonce, Installation string
	Deadline                      int64
}
type ddSessionCall struct {
	Schema, Nonce, ID, Operation, Path string
	Query                              url.Values
	Body                               any
	Mutation                           bool
	Deadline                           int64
}
type ddSessionReply struct {
	Schema, ID string
	Value      any
	Error      *Error
	Exit       int
}
type ddSessionClient struct{ state ddSessionState }

func ddSessionID(id string) bool {
	if len(id) != 51 || !strings.HasPrefix(id, "DD-") {
		return false
	}
	for _, c := range id[3:] {
		if !(c >= '0' && c <= '9' || c >= 'a' && c <= 'f') {
			return false
		}
	}
	return true
}
func ddSessionPath(home, id string) (string, error) {
	if !ddSessionID(id) {
		return "", invalid("$.session", "expected the original DD session ID")
	}
	if !filepath.IsAbs(home) {
		return "", invalid("$.home", "Publish home must be absolute")
	}
	return filepath.Join(home, "publish", "dd", "sessions", id+".json"), nil
}
func ddSaveSession(ctx context.Context, home string, state ddSessionState) error {
	path, err := ddSessionPath(home, state.ID)
	if err != nil {
		return err
	}
	if err = ddPrivateDirectory(filepath.Dir(path)); err != nil {
		return err
	}
	raw, err := json.Marshal(state)
	if err != nil {
		return err
	}
	return vault.ReplaceFile(ctx, path, raw)
}
func ddLoadSession(home, id string) (ddSessionState, error) {
	var state ddSessionState
	path, err := ddSessionPath(home, id)
	if err != nil {
		return state, err
	}
	f, err := os.Open(path)
	if err != nil {
		return state, capability("publish.dd_session", "original DD session record is unavailable")
	}
	defer f.Close()
	raw, err := io.ReadAll(io.LimitReader(f, 65537))
	if err != nil || len(raw) > 65536 || ddJSON(raw, &state) != nil || state.Schema != ddSessionWire || state.ID != id || len(state.Nonce) != 48 || state.Pipe != ddPipeName(id) {
		return state, capability("publish.dd_session", "original DD session record is invalid")
	}
	return state, nil
}
func ddWriteFrame(w io.Writer, value any) error {
	raw, err := json.Marshal(value)
	if err != nil || len(raw) == 0 || len(raw) > ddSessionLimit {
		return ddCCProtocol()
	}
	frame := make([]byte, 4, 4+len(raw))
	binary.LittleEndian.PutUint32(frame, uint32(len(raw)))
	frame = append(frame, raw...)
	_, err = io.Copy(w, bytes.NewReader(frame))
	return err
}
func ddReadFrame(r io.Reader, value any) error {
	return ddReadFrameLimit(r, value, ddSessionLimit)
}
func ddReadFrameLimit(r io.Reader, value any, limit uint32) error {
	var length [4]byte
	if _, err := io.ReadFull(r, length[:]); err != nil {
		return err
	}
	n := binary.LittleEndian.Uint32(length[:])
	if n == 0 || n > limit {
		return ddCCProtocol()
	}
	raw := make([]byte, n)
	if _, err := io.ReadFull(r, raw); err != nil {
		return err
	}
	return ddJSON(raw, value)
}
func ddSessionError(err error) *Error {
	var p *Error
	if errors.As(err, &p) {
		copy := *p
		return &copy
	}
	if errors.Is(err, context.Canceled) {
		return &Error{Code: "command.cancelled", Message: "DD operation was cancelled", Exit: 7}
	}
	if errors.Is(err, context.DeadlineExceeded) {
		return &Error{Code: "publish.dd_deadline", Message: "DD operation reached its original deadline", Exit: 5}
	}
	return &Error{Code: "publish.dd_session", Message: "DD session transport is unavailable; preserve the original request", Exit: 3}
}
func (c *ddSessionClient) exchange(ctx context.Context, call ddSessionCall) (any, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	ctx, cancel := context.WithTimeout(ctx, 5*time.Minute)
	defer cancel()
	deadline, _ := ctx.Deadline()
	call.Schema = ddSessionWire
	call.Nonce = c.state.Nonce
	call.ID = ddRandom()
	call.Deadline = deadline.UnixMilli()
	conn, err := ddDialSession(ctx, c.state)
	if err != nil {
		return nil, ddCCLost()
	}
	defer conn.Close()
	conn.SetDeadline(deadline)
	stop := context.AfterFunc(ctx, func() { conn.Close() })
	defer stop()
	if err = ddWriteFrame(conn, call); err != nil {
		return nil, ddCCLost()
	}
	var reply ddSessionReply
	if err = ddReadFrame(conn, &reply); err != nil {
		return nil, ddCCLost()
	}
	if reply.Schema != ddSessionWire || reply.ID != call.ID {
		return nil, ddCCProtocol()
	}
	if reply.Error != nil {
		reply.Error.Exit = reply.Exit
		if reply.Exit < 2 || reply.Exit > 8 {
			reply.Error.Exit = 5
		}
		return nil, reply.Error
	}
	return reply.Value, nil
}
func (c *ddSessionClient) call(ctx context.Context, path string, q url.Values, body any, mutation bool) (map[string]any, error) {
	v, err := c.exchange(ctx, ddSessionCall{Operation: "call", Path: path, Query: q, Body: body, Mutation: mutation})
	if err != nil {
		return nil, err
	}
	m, ok := v.(map[string]any)
	if !ok {
		return nil, ddCCProtocol()
	}
	return m, nil
}
func (c *ddSessionClient) channels(ctx context.Context) (any, error) {
	return c.exchange(ctx, ddSessionCall{Operation: "channels"})
}
func (c *ddSessionClient) identity() map[string]any { return c.state.Identity }
func (c *ddSessionClient) close() error             { return nil }
func attachDDSession(ctx context.Context, home, id string) (ddAPI, error) {
	state, err := ddLoadSession(home, id)
	if err != nil {
		return nil, err
	}
	if state.Status != "ready" {
		return nil, capability("publish.dd_session", "original DD session is not ready; inspect its status without starting another login")
	}
	client := &ddSessionClient{state: state}
	v, err := client.exchange(ctx, ddSessionCall{Operation: "status"})
	if err != nil {
		return nil, err
	}
	identity, ok := v.(map[string]any)
	if !ok || identity["author_id"] != state.Identity["author_id"] {
		return nil, ddCCProtocol()
	}
	if identity["active"] != true {
		return nil, ddCCLost()
	}
	return client, nil
}
func runDDSession(ctx context.Context, action string, req Request, input map[string]any) (Result, error) {
	result := Result{Platform: "dd", Action: action}
	if err := ctx.Err(); err != nil {
		return result, err
	}
	installation, _ := input["installation"].(string)
	if action == "session doctor" {
		dir, err := ddInstallation(ctx, installation)
		if err != nil {
			return result, err
		}
		result.Complete = true
		result.Data = map[string]any{"installation": dir, "version": "100130", "credentialsRead": false, "sdkLoaded": false}
		return result, nil
	}
	if action == "session start" {
		return ddStartSession(ctx, req, installation)
	}
	id, _ := input["session"].(string)
	state, err := ddLoadSession(req.Home, id)
	if err != nil {
		return result, err
	}
	result.OperationID = id
	if state.Status == "closed" {
		result.Complete = true
		result.Data = map[string]any{"session": id, "status": "closed"}
		return result, nil
	}
	if state.Status != "ready" {
		if action == "session close" {
			alive, e := ddWorkerAlive(state)
			if e == nil && !alive {
				state.Status = "closed"
				if err = ddSaveSession(ctx, req.Home, state); err != nil {
					return result, err
				}
				result.Complete = true
				result.Data = map[string]any{"session": id, "status": "closed"}
				return result, nil
			}
		}
		result.Data = map[string]any{"session": id, "status": state.Status}
		return result, capability("publish.dd_session", "original DD session did not reach ready; no login was replayed")
	}
	client := &ddSessionClient{state: state}
	op := "status"
	if action == "session close" {
		op = "stop"
	}
	value, err := client.exchange(ctx, ddSessionCall{Operation: op})
	if err != nil {
		alive, e := ddWorkerAlive(state)
		if e == nil && !alive {
			state.Status = "ended"
			if op == "stop" {
				state.Status = "closed"
			}
			if e = ddSaveSession(ctx, req.Home, state); e != nil {
				return result, e
			}
			result.Data = map[string]any{"session": id, "status": state.Status}
			if op == "stop" {
				result.Complete = true
				return result, nil
			}
		}
		result.PendingCleanup = alive
		return result, err
	}
	result.Complete = true
	result.Data = map[string]any{"session": id, "status": "ready", "identity": value}
	if op == "stop" {
		result.Data = map[string]any{"session": id, "status": "closed"}
	}
	return result, nil
}
func ddStartSession(ctx context.Context, req Request, installation string) (Result, error) {
	result := Result{Platform: "dd", Action: "session start"}
	if !filepath.IsAbs(req.Home) {
		return result, invalid("$.home", "Publish home must be absolute")
	}
	ctx, cancel := context.WithTimeout(ctx, 90*time.Second)
	defer cancel()
	dir, err := ddInstallation(ctx, installation)
	if err != nil {
		return result, err
	}
	id := "DD-" + ddRandom()
	nonce := ddRandom()
	result.OperationID = id
	state := ddSessionState{Schema: ddSessionWire, ID: id, Nonce: nonce, Pipe: ddPipeName(id), Status: "starting"}
	if err = ddSaveSession(ctx, req.Home, state); err != nil {
		return result, err
	}
	executable, err := os.Executable()
	if err != nil {
		return result, err
	}
	cmd := exec.Command(executable, ddWorkerFlag)
	ddConfigureWorker(cmd)
	stdin, err := cmd.StdinPipe()
	if err != nil {
		return result, err
	}
	stdout, err := cmd.StdoutPipe()
	if err != nil {
		stdin.Close()
		return result, err
	}
	cmd.Stderr = io.Discard
	if err = cmd.Start(); err != nil {
		stdin.Close()
		stdout.Close()
		return result, capability("publish.dd_session_worker", "DD session worker could not start")
	}
	keep := false
	defer func() {
		stdin.Close()
		stdout.Close()
		if !keep {
			cmd.Process.Kill()
		}
		go cmd.Wait()
	}()
	deadline, _ := ctx.Deadline()
	open := ddSessionOpen{Home: req.Home, ID: id, Nonce: nonce, Installation: dir, Deadline: deadline.UnixMilli()}
	stop := context.AfterFunc(ctx, func() {
		cmd.Process.Kill()
		stdout.Close()
	})
	defer stop()
	if err = ddWriteFrame(stdin, open); err != nil {
		return result, ddCCLost()
	}
	stdin.Close()
	var reply ddSessionReply
	if err = ddReadFrame(stdout, &reply); err != nil {
		if ctx.Err() != nil {
			return result, ctx.Err()
		}
		return result, ddCCLost()
	}
	if reply.Schema != ddSessionWire || reply.ID != id {
		return result, ddCCProtocol()
	}
	if reply.Error != nil {
		reply.Error.Exit = reply.Exit
		return result, reply.Error
	}
	saved, err := ddLoadSession(req.Home, id)
	if err != nil {
		return result, err
	}
	if saved.Status != "ready" || saved.PID != cmd.Process.Pid {
		return result, ddCCProtocol()
	}
	if err = ctx.Err(); err != nil {
		return result, err
	}
	if !stop() {
		return result, ctx.Err()
	}
	keep = true
	result.Complete = true
	result.Data = map[string]any{"session": id, "status": "ready", "identity": saved.Identity}
	return result, nil
}

// HandleDDSessionWorker handles only the private process entrypoint. The normal
// CLI calls it before argument parsing; no worker is started during discovery.
func HandleDDSessionWorker(ctx context.Context, args []string) (bool, int) {
	if handled, exit := ddHandleChannelHelper(ctx, args); handled {
		return true, exit
	}
	if len(args) != 1 || args[0] != ddWorkerFlag {
		return false, 0
	}
	err := ddSessionWorker(ctx, os.Stdin, os.Stdout, ddOpenTaskSession)
	if err != nil {
		return true, ddSessionError(err).Exit
	}
	return true, 0
}
func ddSessionWorker(ctx context.Context, in io.Reader, out io.Writer, openAPI func(context.Context, string, string) (ddAPI, error)) error {
	var opening ddSessionOpen
	if ddReadFrame(in, &opening) != nil {
		return ddCCProtocol()
	}
	if !ddSessionID(opening.ID) || len(opening.Nonce) != 48 || opening.Deadline <= time.Now().UnixMilli() || opening.Deadline > time.Now().Add(91*time.Second).UnixMilli() {
		return ddCCProtocol()
	}
	ctx, startCancel := context.WithDeadline(ctx, time.UnixMilli(opening.Deadline))
	defer startCancel()
	// The worker owns synchronous vendor calls. A native call that does not
	// return cannot outlive the original startup deadline or retain credentials.
	startupWatch := time.AfterFunc(time.Until(time.UnixMilli(opening.Deadline)), func() { os.Exit(5) })
	defer startupWatch.Stop()
	state, err := ddLoadSession(opening.Home, opening.ID)
	if err != nil {
		return err
	}
	if state.Status != "starting" || subtle.ConstantTimeCompare([]byte(state.Nonce), []byte(opening.Nonce)) != 1 {
		return ddCCProtocol()
	}
	reply := func(value any, e error) {
		r := ddSessionReply{Schema: ddSessionWire, ID: opening.ID, Value: value}
		if e != nil {
			r.Error = ddSessionError(e)
			r.Exit = r.Error.Exit
		}
		ddWriteFrame(out, r)
	}
	state.PID = os.Getpid()
	state.Created, err = ddProcessCreated(state.PID)
	if err != nil {
		reply(nil, err)
		return err
	}
	if err = ddSaveSession(ctx, opening.Home, state); err != nil {
		reply(nil, err)
		return err
	}
	api, err := openAPI(ctx, opening.Home, opening.Installation)
	if err != nil {
		state.Status = "failed"
		ddSaveSession(context.Background(), opening.Home, state)
		reply(nil, err)
		return err
	}
	defer api.close()
	listener, err := ddListenSession(state)
	if err != nil {
		reply(nil, err)
		return err
	}
	defer listener.Close()
	state.Status = "ready"
	state.Identity = api.identity()
	state.Identity["session"] = state.ID
	if err = ddSaveSession(ctx, opening.Home, state); err != nil {
		reply(nil, err)
		return err
	}
	reply(state.Identity, nil)
	startupWatch.Stop()
	startCancel()
	var mu sync.Mutex
	idle := time.AfterFunc(10*time.Minute, func() { listener.Close() })
	defer idle.Stop()
	for {
		conn, e := listener.Accept()
		if e != nil {
			state.Status = "closed"
			return ddSaveSession(context.Background(), opening.Home, state)
		}
		mu.Lock()
		idle.Stop()
		stop := ddServeSessionBounded(conn, state, api, func() { os.Exit(5) }, func() error {
			state.Status = "closed"
			return ddSaveSession(context.Background(), opening.Home, state)
		})
		conn.Close()
		mu.Unlock()
		if stop {
			return nil
		}
		idle.Reset(10 * time.Minute)
	}
}
func ddServeSessionBounded(conn net.Conn, state ddSessionState, api ddAPI, abort func(), saveClosed func() error) bool {
	conn.SetDeadline(time.Now().Add(15 * time.Second))
	var call ddSessionCall
	if ddReadFrame(conn, &call) != nil {
		return false
	}
	if call.Schema != ddSessionWire || call.ID == "" || subtle.ConstantTimeCompare([]byte(call.Nonce), []byte(state.Nonce)) != 1 {
		return false
	}
	reply := ddSessionReply{Schema: ddSessionWire, ID: call.ID}
	deadline := time.UnixMilli(call.Deadline)
	var err error
	if deadline.Before(time.Now()) || deadline.After(time.Now().Add(5*time.Minute)) {
		err = ddCCProtocol()
	} else {
		conn.SetDeadline(deadline)
		if abort != nil {
			watch := time.AfterFunc(time.Until(deadline)+time.Second, abort)
			defer watch.Stop()
		}
		ctx, cancel := context.WithDeadline(context.Background(), deadline)
		defer cancel()
		switch call.Operation {
		case "status":
			reply.Value = api.identity()
		case "call":
			reply.Value, err = api.call(ctx, call.Path, call.Query, call.Body, call.Mutation)
		case "channels":
			reply.Value, err = api.channels(ctx)
		case "stop":
			err = api.close()
			// The worker is the sole writer while alive. Persist before the
			// acknowledgement so the client never races another replacement.
			if err == nil && saveClosed != nil {
				err = saveClosed()
			}
			reply.Value = map[string]any{"status": "closed"}
		default:
			err = ddCCProtocol()
		}
	}
	if err != nil {
		reply.Error = ddSessionError(err)
		reply.Exit = reply.Error.Exit
	}
	ddWriteFrame(conn, reply)
	return call.Operation == "stop" && err == nil
}
