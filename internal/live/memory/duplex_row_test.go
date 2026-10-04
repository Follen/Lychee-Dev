package memory

import (
	"bytes"
	"context"
	"encoding/binary"
	"errors"
	"io"
	"math"
	"strings"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/live/duplex"
)

func commandRowMessage(t *testing.T, source []byte) duplex.Message {
	t.Helper()
	id := duplex.Identity{Runtime: strings.Repeat("a", 32), Arena: strings.Repeat("b", 32), Session: strings.Repeat("c", 32), Owner: strings.Repeat("d", 32), ActorBinding: strings.Repeat("e", 32), Fence: 1}
	frames, err := duplex.NewFrames(id, strings.Repeat("f", 32), 1, 1, 1234, 1000, source)
	if err != nil || len(frames) != 1 {
		t.Fatal(err, len(frames))
	}
	return frames[0]
}

func numberRow(words int) *duplexRow {
	r := &duplexRow{frozen: true, image: make([]byte, words*24)}
	for i := 0; i < words; i++ {
		cell := r.image[i*24 : (i+1)*24]
		binary.LittleEndian.PutUint64(cell, math.Float64bits(float64(math.MaxUint32)))
		cell[8] = 3
		for j := 10; j < 24; j++ {
			cell[j] = byte(i + j)
		}
	}
	return r
}

func TestWholeCommandRowPreservesMetadataAndClearsLongTail(t *testing.T) {
	for _, size := range []int{0, 3, duplex.MaxSourceBytes} {
		m := commandRowMessage(t, bytes.Repeat([]byte{0x93}, size))
		r := numberRow(duplexCommandWords)
		original := bytes.Clone(r.image)
		plan, err := planDuplexRow(m, r)
		if err != nil {
			t.Fatal(err)
		}
		if len(plan.image) != 6293376 || !bytes.Equal(original, r.image) {
			t.Fatal("logical full row extent or input changed")
		}
		wire, err := duplex.EncodeMessage(m)
		if err != nil {
			t.Fatal(err)
		}
		decoded := make([]byte, duplexCommandWords*4)
		for i := 0; i < duplexCommandWords; i++ {
			cell := plan.image[i*24 : (i+1)*24]
			if !bytes.Equal(cell[8:], original[i*24+8:(i+1)*24]) {
				t.Fatalf("nonpayload changed at record %d", i)
			}
			binary.LittleEndian.PutUint32(decoded[i*4:], uint32(math.Float64frombits(binary.LittleEndian.Uint64(cell))))
		}
		if !bytes.Equal(decoded[:len(wire)], wire) || bytes.Count(decoded[len(wire):], []byte{0}) != len(decoded)-len(wire) {
			t.Fatal("command body or previous long tail not replaced exactly")
		}
	}
}

func TestWholeCommandRowRejectsUnsupportedValuesBeforeWrite(t *testing.T) {
	m := commandRowMessage(t, []byte("return 1"))
	for _, change := range []func(*duplexRow){
		func(r *duplexRow) { r.frozen = false },
		func(r *duplexRow) { r.image = r.image[:len(r.image)-24] },
		func(r *duplexRow) { r.image[len(r.image)-16] = 4 },
		func(r *duplexRow) { r.image[9] = 1 },
		func(r *duplexRow) { binary.LittleEndian.PutUint64(r.image, math.Float64bits(math.NaN())) },
	} {
		r := numberRow(duplexCommandWords)
		change(r)
		if _, err := planDuplexRow(m, r); err == nil {
			t.Fatal("unsupported frozen row accepted")
		}
	}
}

func TestWholeRowSingleAttemptAndFailureOutcomes(t *testing.T) {
	plan, err := planDuplexRow(commandRowMessage(t, []byte("return 1")), numberRow(duplexCommandWords))
	if err != nil {
		t.Fatal(err)
	}
	cases := []struct {
		name string
		mode string
		want duplex.WriteState
	}{
		{"success", "success", duplex.CompleteWrite},
		{"reload-before-write", "guard", duplex.NoWrite},
		{"cancelled-by-guard", "cancel", duplex.NoWrite},
		{"syscall-zero-error-unknown", "zero", duplex.UnknownWrite},
		{"short-write", "short", duplex.PartialWrite},
		{"readback-short", "readshort", duplex.PartialWrite},
		{"readback-mixed-body", "mixed", duplex.PartialWrite},
		{"root-changed-after-write", "root", duplex.PartialWrite},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			ctx, cancel := context.WithTimeout(context.Background(), time.Second)
			defer cancel()
			calls := 0
			before := func(context.Context) error {
				if tc.mode == "guard" {
					return ErrReloadActive
				}
				if tc.mode == "cancel" {
					cancel()
				}
				return nil
			}
			write := func(b []byte) (int, error) {
				calls++
				if !bytes.Equal(b, plan.image) {
					t.Fatal("split or changed image")
				}
				if tc.mode == "zero" {
					return 0, errors.New("injected WPM failure")
				}
				if tc.mode == "short" {
					return len(b) / 2, nil
				}
				return len(b), nil
			}
			read := func(_ context.Context, b []byte) (int, error) {
				copy(b, plan.image)
				if tc.mode == "readshort" {
					return len(b) - 1, io.ErrUnexpectedEOF
				}
				if tc.mode == "mixed" {
					b[len(b)-1] ^= 1
				}
				return len(b), nil
			}
			after := func(context.Context) error {
				if tc.mode == "root" {
					return mailboxError("path_changed")
				}
				return nil
			}
			out, _, err := executeDuplexRow(ctx, plan, before, write, read, after)
			wantCalls := 1
			if tc.want == duplex.NoWrite {
				wantCalls = 0
			}
			if out.State != tc.want || calls != wantCalls || out.ReadbackVerified != (tc.want == duplex.CompleteWrite) || (err == nil) != (tc.want == duplex.CompleteWrite) {
				t.Fatalf("out=%+v calls=%d error=%v", out, calls, err)
			}
		})
	}
}

func TestCommandRowDedicatedBudgetDoesNotWidenGenericReads(t *testing.T) {
	f, r, table := duplexFixture(t)
	address, image := f.allocate(duplexCommandWords * 24)
	copy(image, numberRow(duplexCommandWords).image)
	h := f.bytes(table, 72)
	binary.LittleEndian.PutUint64(h[32:40], address)
	binary.LittleEndian.PutUint32(h[64:68], duplexCommandWords)
	h[0x45] = 1
	a := &luaAccess{reader: r}
	if _, _, err := a.arrayRange(context.Background(), table, 1, duplexCommandWords); err == nil {
		t.Fatal("generic array budget was widened")
	}
	if _, err := a.read(context.Background(), address, len(image), false); err == nil {
		t.Fatal("generic read budget was widened")
	}
	row, err := r.resolveDuplexRow(context.Background(), []DuplexPath{{Name: "inbox"}, {Name: "cells"}}, strings.Repeat("a", 32), strings.Repeat("b", 32), duplexCommandWords)
	if err != nil || !row.frozen || len(row.image) != len(image) {
		t.Fatal("dedicated full row snapshot failed", err)
	}
	h[0x45] = 0
	if err := row.a.verify(context.Background()); err == nil {
		t.Fatal("freeze mutation escaped locating proof")
	}
}
