package memory

import (
	"context"
	"encoding/binary"
	"encoding/hex"
	"errors"
	"fmt"
	"testing"

	"github.com/follenfang/lycheedev/internal/bridge"
)

// Shared fixture for parser and future protocol integration tests. It represents
// separate image/private address spaces and complete Lua hash tables, not a
// fake lookup result. Old allocations remain readable after a relocation.
type mailboxFixture struct {
	t                                          *testing.T
	base, next, state                          uint64
	rootBytes                                  [8]byte
	spans                                      []mailboxFixtureSpan
	nodes                                      map[string]uint64
	tables                                     map[string]uint64
	strings                                    map[string]uint64
	selectors                                  map[string]Selector
	payloads                                   map[string][]byte
	rootReads, privateReads, verifies, regions int
	requested                                  uint64
	reads                                      []mailboxFixtureRead
	beforeRead                                 func(uint64, int)
	verifyError                                func(int) error
}
type mailboxFixtureSpan struct {
	address uint64
	data    []byte
}
type mailboxFixtureRead struct {
	address uint64
	size    int
	root    bool
}
type mailboxFixtureEntry struct {
	name  string
	value luaValue
}

func newMailboxFixture(t *testing.T, moduleBase, heapBase uint64) *mailboxFixture {
	t.Helper()
	f := &mailboxFixture{t: t, base: moduleBase, next: heapBase, nodes: map[string]uint64{}, tables: map[string]uint64{}, strings: map[string]uint64{}, selectors: map[string]Selector{}, payloads: map[string][]byte{}}
	f.install(heapBase)
	return f
}
func (f *mailboxFixture) allocate(size int) (uint64, []byte) {
	address := f.next
	f.next += (uint64(size) + 15) &^ uint64(15)
	b := make([]byte, size)
	f.spans = append(f.spans, mailboxFixtureSpan{address, b})
	return address, b
}
func (f *mailboxFixture) bytes(address uint64, size int) []byte {
	for _, span := range f.spans {
		if address >= span.address && address+uint64(size) >= address && address+uint64(size) <= span.address+uint64(len(span.data)) {
			return span.data[address-span.address : address-span.address+uint64(size)]
		}
	}
	f.t.Fatalf("fixture address not allocated: %x/%d", address, size)
	return nil
}
func (f *mailboxFixture) text(text []byte) uint64 {
	at, b := f.allocate(32 + len(text))
	b[16] = 4
	binary.LittleEndian.PutUint32(b[20:24], luaHash(string(text)))
	binary.LittleEndian.PutUint64(b[24:32], uint64(len(text)))
	copy(b[32:], text)
	return at
}
func (f *mailboxFixture) table(label string, entries []mailboxFixtureEntry) uint64 {
	at, h := f.allocate(72)
	h[16] = 5
	h[19] = 7
	nodes, b := f.allocate(128 * 56)
	binary.LittleEndian.PutUint64(h[40:48], nodes)
	used := map[int]bool{}
	for _, entry := range entries {
		index := int(luaHash(entry.name) & 127)
		if used[index] {
			last := index
			for next := binary.LittleEndian.Uint64(b[last*56+48 : last*56+56]); next != 0; next = binary.LittleEndian.Uint64(b[last*56+48 : last*56+56]) {
				last = int((next - nodes) / 56)
			}
			for index = 0; used[index]; index++ {
			}
			binary.LittleEndian.PutUint64(b[last*56+48:last*56+56], nodes+uint64(index*56))
		}
		used[index] = true
		key := f.text([]byte(entry.name))
		node := b[index*56 : (index+1)*56]
		binary.LittleEndian.PutUint64(node[:8], entry.value.pointer)
		node[8] = entry.value.tag
		binary.LittleEndian.PutUint64(node[24:32], key)
		node[32] = 4
		f.nodes[label+"."+entry.name] = nodes + uint64(index*56)
	}
	f.tables[label] = at
	return at
}
func mailboxFixtureToken(value byte) (out [16]byte) {
	for i := range out {
		out[i] = value
	}
	return
}
func (f *mailboxFixture) record(label string, kind bridge.MemoryKind, nonce [16]byte) uint64 {
	runtime, ticket := mailboxFixtureToken(2), mailboxFixtureToken(3)
	if kind == bridge.MemoryIdentity || kind == bridge.MemoryInputState {
		nonce = runtime
		ticket = [16]byte{}
	}
	payload := []byte(fmt.Sprintf(`{"fixture":"%s"}`, label))
	h := bridge.MemoryHeader{Kind: kind, Runtime: runtime, Nonce: nonce, Ticket: ticket, State: 1, Sequence: 7}
	if kind == bridge.MemoryBody {
		h.State = 3
	}
	wire, err := bridge.EncodeMemoryRecord(h, payload)
	if err != nil {
		f.t.Fatal(err)
	}
	decoded, _, err := bridge.DecodeMemoryRecord(wire)
	if err != nil {
		f.t.Fatal(err)
	}
	f.selectors[label] = Selector{Kind: kind, Runtime: runtime, Nonce: nonce, Ticket: ticket}
	if kind == bridge.MemoryBody {
		s := f.selectors[label]
		s.BodyAuthorized = true
		s.BodyLength = decoded.Length
		s.BodyChecksum = decoded.Checksum
		f.selectors[label] = s
	}
	f.payloads[label] = payload
	address := f.text(wire)
	f.strings[label] = address
	return address
}
func (f *mailboxFixture) install(heapBase uint64) {
	f.next = heapBase
	identity := f.record("identity", bridge.MemoryIdentity, [16]byte{})
	input := f.record("input", bridge.MemoryInputState, [16]byte{})
	receiptNonce, confirmationNonce := mailboxFixtureToken(4), mailboxFixtureToken(5)
	receipt := f.record("receipt", bridge.MemoryReceipt, receiptNonce)
	confirmation := f.record("confirmation", bridge.MemoryConfirmation, confirmationNonce)
	body := f.record("body", bridge.MemoryBody, receiptNonce)
	receipts := f.table("receipts", []mailboxFixtureEntry{{hex.EncodeToString(receiptNonce[:]), luaValue{receipt, 4}}, {hex.EncodeToString(confirmationNonce[:]), luaValue{confirmation, 4}}})
	ticket := mailboxFixtureToken(3)
	bodies := f.table("bodies", []mailboxFixtureEntry{{hex.EncodeToString(ticket[:]), luaValue{body, 4}}})
	runtime := mailboxFixtureToken(2)
	box := f.table("box", []mailboxFixtureEntry{{"schema", luaValue{f.text([]byte(MailboxSchema)), 4}}, {"release", luaValue{f.text([]byte("fixture-release")), 4}}, {"runtime", luaValue{f.text([]byte(hex.EncodeToString(runtime[:]))), 4}}, {"identity", luaValue{identity, 4}}, {"input", luaValue{input, 4}}, {"receipts", luaValue{receipts, 5}}, {"bodies", luaValue{bodies, 5}}})
	ns := f.table("namespace", []mailboxFixtureEntry{{"Mailbox", luaValue{box, 5}}})
	globals := f.table("globals", []mailboxFixtureEntry{{"LycheeDevInternal", luaValue{ns, 5}}})
	state, b := f.allocate(0xb0)
	b[16] = 8
	binary.LittleEndian.PutUint64(b[0x90:0x98], globals)
	b[0x98] = 5
	f.state = state
	binary.LittleEndian.PutUint64(f.rootBytes[:], state)
}
func (f *mailboxFixture) reader() *MailboxReader {
	return &MailboxReader{source: f, root: f.base + RetailLuaMailboxRootRVA, release: "fixture-release", readRoot: f.readRoot}
}
func (f *mailboxFixture) Verify(ctx context.Context) error {
	f.verifies++
	if err := ctx.Err(); err != nil {
		return err
	}
	if f.verifyError != nil {
		return f.verifyError(f.verifies)
	}
	return nil
}
func (f *mailboxFixture) Regions(context.Context) ([]Region, error) {
	f.regions++
	return nil, errors.New("fixture.region_enumeration_forbidden")
}
func (f *mailboxFixture) Read(ctx context.Context, address uint64, b []byte) (int, error) {
	if err := ctx.Err(); err != nil {
		return 0, err
	}
	f.privateReads++
	f.requested += uint64(len(b))
	f.reads = append(f.reads, mailboxFixtureRead{address, len(b), false})
	if f.beforeRead != nil {
		f.beforeRead(address, f.privateReads)
	}
	for _, span := range f.spans {
		if address >= span.address && address+uint64(len(b)) >= address && address+uint64(len(b)) <= span.address+uint64(len(span.data)) {
			copy(b, span.data[address-span.address:])
			return len(b), nil
		}
	}
	return 0, errors.New("fixture.private_read_outside")
}
func (f *mailboxFixture) readRoot(ctx context.Context, address uint64, b []byte) (int, error) {
	if err := ctx.Err(); err != nil {
		return 0, err
	}
	if address != f.base+RetailLuaMailboxRootRVA || len(b) != 8 {
		return 0, errors.New("fixture.module_read_outside")
	}
	f.rootReads++
	f.requested += 8
	f.reads = append(f.reads, mailboxFixtureRead{address, len(b), true})
	copy(b, f.rootBytes[:])
	return 8, nil
}
