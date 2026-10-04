package memory

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"errors"
	"fmt"
	"io"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
)

// These layouts are bound to the recorded runtime-code/IDA investigation, not
// inferred from stock Lua or automatically inherited by another executable.
const RetailLuaMailboxExecutableSHA256 = "d41f11de411f6fdb280a7c1c6380ba3d1b17614f03e5aca8fabce2cc715cd7dd"
const RetailLuaMailboxRootRVA uint64 = 0x79c0c18
const MailboxSchema = "lycheedev.mailbox.v1"

var ErrMailboxUnavailable = errors.New("memory.mailbox_unavailable")
var ErrMailboxPathChanged = fmt.Errorf("%w: path_changed", ErrMailboxUnavailable)

// MailboxReader locates current records through a build-bound Lua root and
// named hash lookup. It stores no heap addresses, hints, payloads or authority.
type MailboxReader struct {
	source   Source
	root     uint64
	readRoot func(context.Context, uint64, []byte) (int, error)
	release  string
	binding  LuaRootBinding
}

// OpenLuaMailbox derives the root from validated runtime code, then leaves Lua
// layout and current publication validation to each bounded lookup. A new hash
// never inherits the previous executable's numeric root RVA.
func OpenLuaMailbox(ctx context.Context, source Source, moduleBase, moduleSize uint64, executableSHA256, release string, layout LuaRootImageLayout, readModule func(context.Context, uint64, []byte) (int, error)) (*MailboxReader, error) {
	if source == nil || readModule == nil || release == "" {
		return nil, mailboxError("reader_configuration")
	}
	if err := source.Verify(ctx); err != nil {
		return nil, err
	}
	binding, err := ResolveLuaMailboxRoot(ctx, moduleBase, moduleSize, executableSHA256, layout, readModule)
	if err != nil {
		return nil, err
	}
	if !binding.validFor(moduleBase, moduleSize, executableSHA256) {
		return nil, mailboxError("root_binding")
	}
	if err := source.Verify(ctx); err != nil {
		return nil, err
	}
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	return &MailboxReader{source: source, root: moduleBase + binding.RootRVA, readRoot: readModule, release: release, binding: binding}, nil
}

func (r *MailboxReader) Binding() LuaRootBinding { return r.binding }

type luaValue struct {
	pointer uint64
	tag     byte
}
type luaGuard struct {
	address uint64
	bytes   []byte
	root    bool
}
type luaAccess struct {
	reader       *MailboxReader
	guards       []luaGuard
	calls, count uint64
	readError    error
}

func mailboxError(reason string) error { return fmt.Errorf("%w: %s", ErrMailboxUnavailable, reason) }

func (a *luaAccess) read(ctx context.Context, address uint64, size int, root bool) ([]byte, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if address == 0 || size < 1 || address > ^uint64(0)-uint64(size) {
		return nil, mailboxError("address")
	}
	if a.calls >= 512 || a.count+uint64(size) > 1<<20 {
		return nil, mailboxError("read_budget")
	}
	a.calls++
	a.count += uint64(size)
	b := make([]byte, size)
	var n int
	var err error
	if root {
		n, err = a.reader.readRoot(ctx, address, b)
	} else {
		n, err = a.reader.source.Read(ctx, address, b)
	}
	if err != nil || n != size {
		if n != size && err == nil {
			err = io.ErrUnexpectedEOF
		}
		a.readError = errors.Join(mailboxError("short_read"), err, ctx.Err())
		return nil, a.readError
	}
	return b, nil
}
func (a *luaAccess) guard(address uint64, b []byte, root bool) {
	a.guards = append(a.guards, luaGuard{address, bytes.Clone(b), root})
}
func (a *luaAccess) verify(ctx context.Context) error {
	for _, g := range a.guards {
		b, err := a.read(ctx, g.address, len(g.bytes), g.root)
		if err != nil {
			return err
		}
		if !bytes.Equal(b, g.bytes) {
			return ErrMailboxPathChanged
		}
	}
	if err := a.reader.source.Verify(ctx); err != nil {
		return err
	}
	return ctx.Err()
}
func luaHash(text string) uint32 {
	b := []byte(text)
	h := uint32(len(b))
	step := (len(b) >> 5) + 1
	for n := len(b); n >= step; n -= step {
		h ^= (h << 5) + (h >> 2) + uint32(b[n-1])
	}
	return h
}
func (a *luaAccess) string(ctx context.Context, address uint64, maximum uint64, guard bool) ([]byte, error) {
	h, err := a.read(ctx, address, 32, false)
	if err != nil {
		return nil, err
	}
	length := binary.LittleEndian.Uint64(h[24:32])
	if h[16] != 4 || length > maximum || address > ^uint64(0)-32-length {
		return nil, mailboxError("string_layout")
	}
	var b []byte
	if length != 0 {
		b, err = a.read(ctx, address+32, int(length), false)
		if err != nil {
			return nil, err
		}
	}
	if guard {
		a.guard(address+16, h[16:17], false)
		a.guard(address+20, h[20:32], false)
		if length != 0 {
			a.guard(address+32, b, false)
		}
	}
	return b, nil
}
func (a *luaAccess) lookup(ctx context.Context, table uint64, name string) (luaValue, error) {
	h, err := a.read(ctx, table, 72, false)
	if err != nil {
		return luaValue{}, err
	}
	if h[16] != 5 || h[19] > 20 {
		return luaValue{}, mailboxError("table_layout")
	}
	nodes := binary.LittleEndian.Uint64(h[40:48])
	count := uint64(1) << h[19]
	if nodes == 0 || nodes > ^uint64(0)-count*56 {
		return luaValue{}, mailboxError("node_range")
	}
	a.guard(table+16, h[16:17], false)
	a.guard(table+19, h[19:20], false)
	a.guard(table+40, h[40:48], false)
	node := nodes + uint64(luaHash(name)&uint32(count-1))*56
	seen := map[uint64]bool{}
	for i := 0; i < 64 && node != 0; i++ {
		if node < nodes || node >= nodes+count*56 || (node-nodes)%56 != 0 || seen[node] {
			return luaValue{}, mailboxError("collision_chain")
		}
		seen[node] = true
		b, err := a.read(ctx, node, 56, false)
		if err != nil {
			return luaValue{}, err
		}
		a.guard(node+24, b[24:34], false)
		if b[32] == 4 {
			if b[33] != 0 {
				return luaValue{}, mailboxError("restricted_key")
			}
			key, err := a.string(ctx, binary.LittleEndian.Uint64(b[24:32]), 256, true)
			if err != nil {
				return luaValue{}, err
			}
			if string(key) == name {
				if b[9] != 0 {
					return luaValue{}, mailboxError("restricted_value")
				}
				a.guard(node, b[:10], false)
				return luaValue{binary.LittleEndian.Uint64(b[:8]), b[8]}, nil
			}
		}
		a.guard(node+48, b[48:56], false)
		node = binary.LittleEndian.Uint64(b[48:56])
	}
	if node != 0 {
		return luaValue{}, mailboxError("collision_limit")
	}
	return luaValue{}, nil
}

// Lookup covers only the declared current mailbox field/key. Coverage never
// claims a full heap scan. Existing selector and wire validators retain BODY
// authorization, exact nonces/tickets/runtime and caller freshness predicates.
func (r *MailboxReader) Lookup(ctx context.Context, selector Selector) (out LookupResult, err error) {
	started := time.Now()
	a := &luaAccess{reader: r}
	out = LookupResult{Path: "lua_mailbox"}
	defer func() {
		out.ElapsedMillis = time.Since(started).Milliseconds()
		out.Coverage = Coverage{Complete: err == nil, Gaps: []Gap{}, Workers: []Worker{{Complete: err == nil, ReadCalls: a.calls, ScannedBytes: a.count, PlannedBytes: a.count, Gaps: []Gap{}}}, ScannedBytes: a.count, PlannedBytes: a.count, ElapsedMillis: out.ElapsedMillis}
	}()
	if err = r.source.Verify(ctx); err != nil {
		return out, err
	}
	var b []byte
	if r.binding.proof != nil {
		anchor := r.root - r.binding.RootRVA + r.binding.AnchorRVA
		b, err = a.read(ctx, anchor, r.binding.Evidence.PatternBytes, true)
		if err != nil {
			return out, err
		}
		hash := sha256.Sum256(b)
		if hex.EncodeToString(hash[:]) != r.binding.Evidence.AnchorSHA256 {
			return out, mailboxError("root_recipe_changed")
		}
		a.guard(anchor, b, true)
	}
	b, err = a.read(ctx, r.root, 8, true)
	if err != nil {
		return out, err
	}
	a.guard(r.root, b, true)
	state := binary.LittleEndian.Uint64(b)
	if state == 0 {
		return out, mailboxError("lua_state_null")
	}
	b, err = a.read(ctx, state+16, 1, false)
	if err != nil {
		return out, err
	}
	if b[0] != 8 {
		return out, mailboxError("lua_state_tag")
	}
	a.guard(state+16, b, false)
	b, err = a.read(ctx, state+0x90, 24, false)
	if err != nil {
		return out, err
	}
	if b[8] != 5 || b[9] != 0 {
		return out, mailboxError("globals_layout")
	}
	a.guard(state+0x90, b[:10], false)
	var ns, box luaValue
	ns, err = a.lookup(ctx, binary.LittleEndian.Uint64(b[:8]), "LycheeDevInternal")
	if err != nil {
		return out, err
	}
	if ns.tag == 0 {
		err = a.verify(ctx)
		return out, err
	}
	if ns.tag != 5 {
		return out, mailboxError("namespace_layout")
	}
	box, err = a.lookup(ctx, ns.pointer, "Mailbox")
	if err != nil {
		return out, err
	}
	if box.tag == 0 {
		err = a.verify(ctx)
		return out, err
	}
	if box.tag != 5 {
		return out, mailboxError("mailbox_layout")
	}
	for _, check := range []struct{ name, want string }{{"schema", MailboxSchema}, {"release", r.release}} {
		var v luaValue
		v, err = a.lookup(ctx, box.pointer, check.name)
		if err != nil {
			return out, err
		}
		if v.tag != 4 {
			return out, mailboxError("mailbox_" + check.name)
		}
		b, err = a.string(ctx, v.pointer, 64, true)
		if err != nil {
			return out, err
		}
		if string(b) != check.want {
			return out, mailboxError("mailbox_" + check.name + "_mismatch")
		}
	}
	var rt luaValue
	rt, err = a.lookup(ctx, box.pointer, "runtime")
	if err != nil {
		return out, err
	}
	if rt.tag != 4 {
		return out, mailboxError("runtime_layout")
	}
	b, err = a.string(ctx, rt.pointer, 32, true)
	if err != nil {
		return out, err
	}
	runtimeBytes, e := hex.DecodeString(string(b))
	if e != nil || len(runtimeBytes) != 16 || string(b) != hex.EncodeToString(runtimeBytes) {
		return out, mailboxError("runtime_token")
	}
	var currentRuntime [16]byte
	copy(currentRuntime[:], runtimeBytes)
	if currentRuntime == ([16]byte{}) {
		return out, mailboxError("runtime_zero")
	}
	if selector.Runtime != ([16]byte{}) && selector.Runtime != currentRuntime {
		err = a.verify(ctx)
		return out, err
	}
	field, key := "", ""
	switch selector.Kind {
	case bridge.MemoryIdentity:
		field = "identity"
	case bridge.MemoryInputState:
		field = "input"
	case bridge.MemoryReceipt, bridge.MemoryConfirmation:
		field = "receipts"
		if selector.Nonce == ([16]byte{}) {
			return out, mailboxError("nonce_required")
		}
		key = hex.EncodeToString(selector.Nonce[:])
	case bridge.MemoryBody:
		field = "bodies"
		if !selector.BodyAuthorized || selector.Nonce == ([16]byte{}) || selector.Runtime == ([16]byte{}) || selector.Ticket == ([16]byte{}) || selector.BodyLength > bridge.MemoryMaxPayload {
			return out, mailboxError("body_authorization")
		}
		key = hex.EncodeToString(selector.Ticket[:])
	default:
		return out, mailboxError("kind_unsupported")
	}
	var value luaValue
	value, err = a.lookup(ctx, box.pointer, field)
	if err != nil {
		return out, err
	}
	if key != "" {
		if value.tag != 5 {
			return out, mailboxError("record_map")
		}
		value, err = a.lookup(ctx, value.pointer, key)
		if err != nil {
			return out, err
		}
	}
	if value.tag == 0 {
		err = a.verify(ctx)
		return out, err
	}
	if value.tag != 4 {
		return out, mailboxError("record_type")
	}
	h, readErr := a.read(ctx, value.pointer, 32, false)
	if readErr != nil {
		return out, readErr
	}
	length := binary.LittleEndian.Uint64(h[24:32])
	maximum := uint64(16<<10) + bridge.MemoryHeaderBytes + bridge.MemoryTrailerBytes
	if selector.Kind == bridge.MemoryBody {
		maximum = uint64(selector.BodyLength) + bridge.MemoryHeaderBytes + bridge.MemoryTrailerBytes
	}
	if h[16] != 4 || length < bridge.MemoryHeaderBytes+bridge.MemoryTrailerBytes || length > maximum || value.pointer > ^uint64(0)-32-length {
		return out, mailboxError("record_string_length")
	}
	a.guard(value.pointer+16, h[16:17], false)
	a.guard(value.pointer+24, h[24:32], false)
	// Caller predicates run only after the complete locating path is rechecked.
	validate := selector
	validate.Accept = nil
	var record Record
	record, readErr = ReadRecord(ctx, &luaRecordSource{a}, value.pointer+32, validate)
	if readErr != nil {
		if err = ctx.Err(); err != nil {
			return out, errors.Join(err, a.readError)
		}
		if readErr.Error() == "memory.record_mismatch" {
			err = a.verify(ctx)
			return out, err
		}
		return out, errors.Join(mailboxError("record_invalid: "+readErr.Error()), a.readError)
	}
	if uint64(len(record.Payload))+bridge.MemoryHeaderBytes+bridge.MemoryTrailerBytes != length || record.Header.Runtime != currentRuntime {
		return out, mailboxError("record_identity")
	}
	if err = a.verify(ctx); err != nil {
		return out, err
	}
	if selector.Accept == nil || selector.Accept(record) {
		out.Records = []Record{record}
	}
	if err = ctx.Err(); err != nil {
		out.Records = nil
		return out, err
	}
	return out, nil
}

type luaRecordSource struct{ a *luaAccess }

func (s *luaRecordSource) Verify(ctx context.Context) error { return s.a.reader.source.Verify(ctx) }
func (*luaRecordSource) Regions(context.Context) ([]Region, error) {
	return nil, mailboxError("region_enumeration_forbidden")
}
func (s *luaRecordSource) Read(ctx context.Context, address uint64, b []byte) (int, error) {
	v, err := s.a.read(ctx, address, len(b), false)
	if err != nil {
		s.a.readError = err
		return 0, err
	}
	copy(b, v)
	return len(b), nil
}
