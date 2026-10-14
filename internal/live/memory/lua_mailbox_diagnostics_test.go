package memory

import (
	"context"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"strings"
	"testing"
)

func TestMailboxFirstReadFailureRetainsBoundedLogicalEvidence(t *testing.T) {
	for _, kind := range []string{"receipt_node", "wire_header", "module_root", "healthy"} {
		t.Run(kind, func(t *testing.T) {
			f := newMailboxFixture(t, 0x140000000, 0x200000000)
			r := f.reader()
			selector := f.selectors["receipt"]
			var fault error = io.ErrUnexpectedEOF
			switch kind {
			case "receipt_node":
				attempted := false
				fault = &readFailureError{cause: errors.New("fixture private content must not enter metadata"), details: ReadFailure{Reason: "region_ineligible", Region: &ReadRegion{Type: 0x1000000, State: 0x1000, Protect: 2}, IOAttempted: &attempted}}
				r.source = mailboxFaultSource{Source: f, at: f.nodes["receipts."+hex.EncodeToString(selector.Nonce[:])], err: fault}
			case "wire_header":
				r.source = mailboxFaultSource{Source: f, at: f.strings["receipt"] + 32, n: 4, err: fault}
			case "module_root":
				r.readRoot = func(context.Context, uint64, []byte) (int, error) { return 3, fault }
			}
			found, err := r.Lookup(context.Background(), selector)
			first := found.FirstReadFailure
			if kind == "healthy" {
				if err != nil || len(found.Records) != 1 || first != nil {
					t.Fatalf("healthy lookup gained failure metadata: %+v %v", first, err)
				}
				return
			}
			if err == nil || len(found.Records) != 0 || first == nil {
				t.Fatalf("failure missing: %+v %v", first, err)
			}
			if kind != "wire_header" && !errors.Is(err, fault) {
				t.Fatal("lost original read error", err)
			}
			switch kind {
			case "receipt_node":
				if first.Path != "record.receipts.entry.node" || first.RequestedBytes != 56 || first.ReturnedBytes != 0 || first.Source != "private" || first.Reason != "region_ineligible" || first.Region == nil || first.Region.Type != 0x1000000 || first.IOAttempted == nil || *first.IOAttempted || first.GuardRecheck != "stable" {
					t.Fatalf("wrong existing OS evidence: %+v", first)
				}
				if found.Coverage.Workers[0].ReadCalls != uint64(f.rootReads+f.privateReads+1) {
					t.Fatal("diagnostics added reads")
				}
			case "wire_header":
				if first.Path != "record.wire_header" || first.ReturnedBytes != 4 || first.Source != "private" || first.Region != nil || first.IOAttempted != nil || first.GuardRecheck != "stable" {
					t.Fatalf("lost short prefix or invented native facts: %+v", first)
				}
			case "module_root":
				if first.Path != "root.state" || first.RequestedBytes != 8 || first.ReturnedBytes != 3 || first.Source != "module" || first.Region != nil || first.IOAttempted != nil || first.GuardRecheck != "no_guards" {
					t.Fatalf("root failure metadata: %+v", first)
				}
			}
			encoded, marshalErr := json.Marshal(first)
			if marshalErr != nil {
				t.Fatal(marshalErr)
			}
			for _, private := range []string{hex.EncodeToString(selector.Nonce[:]), "fixture private content", "address", "payload", "character"} {
				if strings.Contains(string(encoded), private) {
					t.Fatalf("diagnostic exposed private data: %s", encoded)
				}
			}
			if len(encoded) > 512 {
				t.Fatal("first failure evidence is not bounded", len(encoded))
			}
		})
	}
}

func TestMailboxFirstReadFailurePrecedesFailedGuard(t *testing.T) {
	f := newMailboxFixture(t, 0x140000000, 0x200000000)
	r := f.reader()
	selector := f.selectors["receipt"]
	initial := errors.New("fixture receipt node unavailable")
	r.source = mailboxFaultSource{Source: f, at: f.nodes["receipts."+hex.EncodeToString(selector.Nonce[:])], err: initial}
	reads := 0
	r.readRoot = func(ctx context.Context, at uint64, b []byte) (int, error) {
		reads++
		if reads > 1 {
			return 0, errors.New("fixture guard read unavailable")
		}
		return f.readRoot(ctx, at, b)
	}
	found, err := r.Lookup(context.Background(), selector)
	first := found.FirstReadFailure
	if !errors.Is(err, initial) || first == nil || first.Path != "record.receipts.entry.node" || first.RequestedBytes != 56 || first.Source != "private" || first.GuardRecheck != "read_failed" {
		t.Fatalf("guard failure replaced the first failed read: %+v %v", first, err)
	}
}
