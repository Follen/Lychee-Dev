package memory

import (
	"bytes"
	"context"
	"encoding/binary"
	"errors"
	"fmt"
	"io"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/bridge"
)

type mailboxFaultSource struct {
	Source
	at  uint64
	n   int
	err error
}

func TestLuaMailboxRecordReadRetainsPreIOBudgetCause(t *testing.T) {
	f := newMailboxFixture(t, 0x140000000, 0x200000000)
	a := &luaAccess{reader: f.reader(), count: (1 << 20) - bridge.MemoryHeaderBytes + 1}
	_, err := ReadRecord(context.Background(), &luaRecordSource{a}, f.strings["input"]+32, f.selectors["input"])
	if err == nil || !errors.Is(a.readError, ErrMailboxUnavailable) || !strings.Contains(a.readError.Error(), "read_budget") || f.privateReads != 0 || a.calls != 0 {
		t.Fatal("record header lost pre-IO budget cause or reached Source", err, a.readError, f.privateReads, a.calls)
	}
}

func (s mailboxFaultSource) Read(ctx context.Context, at uint64, b []byte) (int, error) {
	if at == s.at {
		return s.n, s.err
	}
	return s.Source.Read(ctx, at, b)
}
func TestLuaMailboxPreservesReadCauses(t *testing.T) {
	for _, surface := range []string{"root", "heap", "record"} {
		for _, mode := range []string{"cause", "short", "cancel"} {
			t.Run(surface+"/"+mode, func(t *testing.T) {
				f := newMailboxFixture(t, 0x140000000, 0x200000000)
				r := f.reader()
				ctx, cancel := context.WithCancel(context.Background())
				defer cancel()
				cause := errors.New("fixture.mapping_or_heap_read_changed")
				n := 0
				var readErr error = cause
				if mode == "short" {
					readErr = nil
					cause = io.ErrUnexpectedEOF
				}
				if mode == "cancel" {
					readErr = context.Canceled
					cause = context.Canceled
				}
				if surface == "root" {
					r.readRoot = func(context.Context, uint64, []byte) (int, error) {
						if mode == "cancel" {
							cancel()
						}
						return n, readErr
					}
				} else {
					at := f.state + 16
					if surface == "record" {
						at = f.strings["input"] + 32
					}
					r.source = mailboxFaultSource{Source: f, at: at, n: n, err: readErr}
				}
				out, err := r.Lookup(ctx, f.selectors["input"])
				if !errors.Is(err, cause) || !errors.Is(err, ErrMailboxUnavailable) || len(out.Records) != 0 || out.Coverage.Complete {
					t.Fatal("read cause erased or authorized", err, out.Coverage.Complete)
				}
			})
		}
	}
}

func TestLuaMailboxNamedRecordsWithoutScanning(t *testing.T) {
	for _, label := range []string{"identity", "input", "receipt", "confirmation", "body"} {
		t.Run(label, func(t *testing.T) {
			f := newMailboxFixture(t, 0x140000000, 0x200000000)
			result, err := f.reader().Lookup(context.Background(), f.selectors[label])
			if err != nil || len(result.Records) != 1 || !bytes.Equal(result.Records[0].Payload, f.payloads[label]) {
				t.Fatalf("named %s lookup failed: records=%d err=%v", label, len(result.Records), err)
			}
			if result.Path != "lua_mailbox" || f.regions != 0 || f.rootReads != 2 || f.verifies != 2 {
				t.Fatal("locator scanned, omitted guards or process verification")
			}
			if result.Records[0].Address != f.strings[label]+32 || result.Coverage.ScannedBytes != f.requested || result.Coverage.Workers[0].ReadCalls != uint64(f.rootReads+f.privateReads) {
				t.Fatal("wrong current record or physical read accounting")
			}
		})
	}
}

func TestLuaMailboxNameHashGolden(t *testing.T) {
	// Independent fixed vectors keep the fixture's table constructor from
	// masking a changed bucket calculation in the production reader.
	for name, want := range map[string]uint32{"LycheeDevInternal": 2117440413, "Mailbox": 550757567, "receipts": 482574166, "02020202020202020202020202020202": 392374290} {
		if got := luaHash(name); got != want {
			t.Fatal("named Lua bucket hash changed", name, got, want)
		}
	}
}

func TestLuaMailboxRootChangesInvalidateTheWholePath(t *testing.T) {
	f := newMailboxFixture(t, 0x140000000, 0x200000000)
	address := f.strings["input"] + 32
	f.beforeRead = func(at uint64, _ int) {
		if at == address {
			binary.LittleEndian.PutUint64(f.rootBytes[:], 0)
		}
	}
	result, err := f.reader().Lookup(context.Background(), f.selectors["input"])
	if !errors.Is(err, ErrMailboxUnavailable) || !strings.Contains(err.Error(), "path_changed") || len(result.Records) != 0 {
		t.Fatal("old root path authorized", err)
	}
}

func TestLuaMailboxMissingPublicationDoesNotScanResidue(t *testing.T) {
	for _, field := range []string{"globals.LycheeDevInternal", "namespace.Mailbox", "box.input"} {
		t.Run(field, func(t *testing.T) {
			f := newMailboxFixture(t, 0x140000000, 0x200000000)
			// All old fully valid protocol records remain allocated/readable.
			f.bytes(f.nodes[field]+8, 1)[0] = 0
			result, err := f.reader().Lookup(context.Background(), f.selectors["input"])
			if err != nil || len(result.Records) != 0 || f.regions != 0 || f.verifies != 2 {
				t.Fatal("absent current publication was recovered from heap residue", err)
			}
		})
	}
}

func TestLuaMailboxMissingFieldCancellationIsIncomplete(t *testing.T) {
	f := newMailboxFixture(t, 0x140000000, 0x200000000)
	f.bytes(f.nodes["box.input"]+8, 1)[0] = 0
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	f.verifyError = func(n int) error {
		if n == 2 {
			cancel()
		}
		return nil
	}
	found, err := f.reader().Lookup(ctx, f.selectors["input"])
	if !errors.Is(err, context.Canceled) || found.Coverage.Complete || len(found.Records) != 0 {
		t.Fatal("cancelled missing-field lookup claimed completion", found, err)
	}
}

func TestLuaMailboxBodyRequiresExactAuthorization(t *testing.T) {
	for _, tc := range []struct {
		name   string
		change func(*Selector)
	}{
		{"unauthorized", func(s *Selector) { s.BodyAuthorized = false }},
		{"nonce", func(s *Selector) { s.Nonce = mailboxFixtureToken(9) }},
		{"runtime", func(s *Selector) { s.Runtime = mailboxFixtureToken(9) }},
		{"ticket", func(s *Selector) { s.Ticket = mailboxFixtureToken(9) }},
		{"length", func(s *Selector) { s.BodyLength++ }},
		{"checksum", func(s *Selector) { s.BodyChecksum++ }},
	} {
		t.Run(tc.name, func(t *testing.T) {
			f := newMailboxFixture(t, 0x140000000, 0x200000000)
			selector := f.selectors["body"]
			tc.change(&selector)
			result, _ := f.reader().Lookup(context.Background(), selector)
			if len(result.Records) != 0 || f.regions != 0 {
				t.Fatal("unauthorized BODY accepted or scan fallback used")
			}
			for _, read := range f.reads {
				if read.address == f.strings["body"]+32 && read.size > bridge.MemoryHeaderBytes {
					t.Fatal("BODY payload read before exact authorization")
				}
			}
		})
	}
}

func TestLuaMailboxFullCollisionPathSharesReadBudget(t *testing.T) {
	f := newMailboxFixture(t, 0x140000000, 0x200000000)
	var entries []mailboxFixtureEntry
	want := luaHash("LycheeDevInternal") & 127
	for i := 0; len(entries) < 56; i++ {
		name := fmt.Sprintf("collision-%d", i)
		if luaHash(name)&127 == want {
			entries = append(entries, mailboxFixtureEntry{name, luaValue{0, 0}})
		}
	}
	entries = append(entries, mailboxFixtureEntry{"LycheeDevInternal", luaValue{f.tables["namespace"], 5}})
	globals := f.table("wide_globals", entries)
	binary.LittleEndian.PutUint64(f.bytes(f.state+0x90, 8), globals)
	selector := f.selectors["input"]
	predicate := false
	selector.Accept = func(Record) bool { predicate = true; return true }
	result, err := f.reader().Lookup(context.Background(), selector)
	if !errors.Is(err, ErrMailboxUnavailable) || !strings.Contains(err.Error(), "read_budget") || len(result.Records) != 0 || predicate {
		t.Fatal("collision/guard verification escaped shared read budget", err)
	}
	if f.rootReads+f.privateReads > 512 || f.requested > 1<<20 || f.regions != 0 {
		t.Fatal("read budget applied after physical IO or scan fallback")
	}
	// The byte cap must also reject a request before allocation or Source IO.
	f = newMailboxFixture(t, 0x140000000, 0x200000000)
	access := luaAccess{reader: f.reader()}
	if _, err := access.read(context.Background(), f.strings["input"], (1<<20)+1, false); !errors.Is(err, ErrMailboxUnavailable) || f.privateReads != 0 {
		t.Fatal("oversized read reached Source", err)
	}
}

func TestLuaMailboxProcessAndMidRecordCancellationReject(t *testing.T) {
	f := newMailboxFixture(t, 0x140000000, 0x200000000)
	identityChanged := errors.New("fixture.process_changed")
	f.verifyError = func(n int) error {
		if n == 2 {
			return identityChanged
		}
		return nil
	}
	result, err := f.reader().Lookup(context.Background(), f.selectors["input"])
	if !errors.Is(err, identityChanged) || len(result.Records) != 0 {
		t.Fatal("final process verification did not gate result", err)
	}
	f = newMailboxFixture(t, 0x140000000, 0x200000000)
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	f.beforeRead = func(address uint64, _ int) {
		if address == f.strings["input"]+32 {
			cancel()
		}
	}
	result, err = f.reader().Lookup(ctx, f.selectors["input"])
	if !errors.Is(err, context.Canceled) || len(result.Records) != 0 {
		t.Fatal("record IO cancellation lost its typed cause", err)
	}
}

func TestLuaMailboxIdentityAndLayoutFailClosed(t *testing.T) {
	for _, tc := range []struct {
		name   string
		mutate func(*mailboxFixture)
	}{
		{"release", func(f *mailboxFixture) {
			node := f.bytes(f.nodes["box.release"], 8)
			binary.LittleEndian.PutUint64(node, f.text([]byte("other-release")))
		}},
		{"schema", func(f *mailboxFixture) {
			node := f.bytes(f.nodes["box.schema"], 8)
			binary.LittleEndian.PutUint64(node, f.text([]byte("lycheedev.mailbox.invalid")))
		}},
		{"runtime", func(f *mailboxFixture) {
			node := f.bytes(f.nodes["box.runtime"], 8)
			binary.LittleEndian.PutUint64(node, f.text([]byte(strings.Repeat("0", 32))))
		}},
		{"secret_key", func(f *mailboxFixture) { f.bytes(f.nodes["globals.LycheeDevInternal"]+33, 1)[0] = 1 }},
		{"secret_value", func(f *mailboxFixture) { f.bytes(f.nodes["namespace.Mailbox"]+9, 1)[0] = 1 }},
		{"thread_tag", func(f *mailboxFixture) { f.bytes(f.state+16, 1)[0] = 5 }},
		{"globals_tag", func(f *mailboxFixture) { f.bytes(f.state+0x98, 1)[0] = 4 }},
		{"table_tag", func(f *mailboxFixture) { f.bytes(f.tables["box"]+16, 1)[0] = 4 }},
		{"table_count", func(f *mailboxFixture) { f.bytes(f.tables["globals"]+19, 1)[0] = 21 }},
		{"string_tag", func(f *mailboxFixture) { f.bytes(f.strings["input"]+16, 1)[0] = 5 }},
		{"string_length", func(f *mailboxFixture) { binary.LittleEndian.PutUint64(f.bytes(f.strings["input"]+24, 8), ^uint64(0)) }},
		{"crc", func(f *mailboxFixture) { f.bytes(f.strings["input"]+32+bridge.MemoryHeaderBytes, 1)[0] ^= 1 }},
	} {
		t.Run(tc.name, func(t *testing.T) {
			f := newMailboxFixture(t, 0x140000000, 0x200000000)
			tc.mutate(f)
			result, err := f.reader().Lookup(context.Background(), f.selectors["input"])
			if !errors.Is(err, ErrMailboxUnavailable) || len(result.Records) != 0 || f.regions != 0 {
				t.Fatal("invalid identity/layout accepted", err)
			}
		})
	}
	f := newMailboxFixture(t, 0x140000000, 0x200000000)
	if _, err := OpenLuaMailbox(context.Background(), f, f.base, 0x8000000, strings.Repeat("0", 64), "fixture-release", LuaRootImageLayout{}, f.readRoot); err == nil {
		t.Fatal("unsupported executable layout enabled")
	}
	if _, err := OpenLuaMailbox(context.Background(), f, ^uint64(0), 0x8000000, RetailLuaMailboxExecutableSHA256, "fixture-release", LuaRootImageLayout{}, f.readRoot); err == nil {
		t.Fatal("overflowing module relocation enabled")
	}
}

func TestLuaMailboxPathChangesBeforePredicateAreUnavailable(t *testing.T) {
	for _, field := range []string{"globals.LycheeDevInternal", "namespace.Mailbox", "box.input"} {
		t.Run(field, func(t *testing.T) {
			f := newMailboxFixture(t, 0x140000000, 0x200000000)
			node := f.nodes[field]
			reads := 0
			f.beforeRead = func(address uint64, _ int) {
				if address == node {
					reads++
					if reads == 2 {
						f.bytes(node, 1)[0] ^= 16
					}
				}
			}
			accepted := false
			selector := f.selectors["input"]
			selector.Accept = func(Record) bool { accepted = true; return true }
			result, err := f.reader().Lookup(context.Background(), selector)
			if !errors.Is(err, ErrMailboxUnavailable) || len(result.Records) != 0 || accepted {
				t.Fatal("changed locating path reached authorization", err)
			}
		})
	}
}

func TestLuaMailboxEachLookupFollowsCurrentHeapAndASLR(t *testing.T) {
	f := newMailboxFixture(t, 0x140000000, 0x200000000)
	reader := f.reader()
	first, err := reader.Lookup(context.Background(), f.selectors["input"])
	if err != nil || len(first.Records) != 1 {
		t.Fatal(err)
	}
	f.install(0x300000000)
	second, err := reader.Lookup(context.Background(), f.selectors["input"])
	if err != nil || len(second.Records) != 1 || second.Records[0].Address == first.Records[0].Address || second.Records[0].Address != f.strings["input"]+32 {
		t.Fatal("retained an old heap locator", err)
	}
	other := newMailboxFixture(t, 0x180000000, 0x400000000)
	result, err := other.reader().Lookup(context.Background(), other.selectors["identity"])
	if err != nil || len(result.Records) != 1 || other.rootReads != 2 || other.regions != 0 {
		t.Fatal("ASLR root was not relocated", err)
	}
}

func TestLuaMailboxPredicatesAndCancellationDoNotAuthorize(t *testing.T) {
	f := newMailboxFixture(t, 0x140000000, 0x200000000)
	selector := f.selectors["input"]
	calls := 0
	selector.Accept = func(Record) bool { calls++; return false }
	result, err := f.reader().Lookup(context.Background(), selector)
	if err != nil || len(result.Records) != 0 || calls != 1 {
		t.Fatal("rejected predicate accepted", err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	before := f.rootReads + f.privateReads
	result, err = f.reader().Lookup(ctx, selector)
	if !errors.Is(err, context.Canceled) || len(result.Records) != 0 || before != f.rootReads+f.privateReads {
		t.Fatal("cancelled locator did work or authorized", err)
	}
	f = newMailboxFixture(t, 0x140000000, 0x200000000)
	ctx, cancel = context.WithCancel(context.Background())
	defer cancel()
	selector = f.selectors["input"]
	selector.Accept = func(Record) bool { cancel(); return true }
	result, err = f.reader().Lookup(ctx, selector)
	if !errors.Is(err, context.Canceled) || len(result.Records) != 0 {
		t.Fatal("predicate-time cancellation authorized record", err)
	}
}
