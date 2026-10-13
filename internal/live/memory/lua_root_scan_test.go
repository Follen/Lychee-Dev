package memory

import (
	"bytes"
	"context"
	"encoding/binary"
	"encoding/hex"
	"errors"
	"io"
	"testing"
)

func TestForeverCapturedAnchorsUseReadableSpansWithoutHashAllowlist(t *testing.T) {
	for _, tc := range []struct {
		name, code   string
		anchor, root uint64
	}{
		{"70124", "488b1d14da4107488bcbe82447fb03488bcb4c8b38e81947fb03498bd7488bcb448b4008e81a74fb03488bcbe80247fb03", 0x75a69d, 0x7b780b8},
		{"asia70338", "488b1d66fb2807488bcbe85613e303488bcb488b28e84b13e303488bd5488bcb448b4008e84c40e303488bcbe83413e303", 0x764b4b, 0x79f46b8},
	} {
		t.Run(tc.name, func(t *testing.T) {
			f := newRootRecipeFixture(t, 0x1000, 4096, tc.root&^4095)
			// Preserve the real captured caller, but deliberately use an unknown
			// executable hash. Only one OS-readable page exists in this fixture.
			binary.LittleEndian.PutUint32(f.header[0x188+8:], 0x4800000-0x1000)
			f.diskHeader = bytes.Clone(f.header)
			page, start := make([]byte, 4096), tc.anchor&^4095
			code, _ := hex.DecodeString(tc.code)
			copy(page[tc.anchor-start:], code)
			f.hook = func(at uint64, b []byte) (int, error, bool) {
				if at >= start && at+uint64(len(b)) <= start+4096 {
					return copy(b, page[at-start:]), nil, true
				}
				if at >= f.textRVA && at < 0x4800000 {
					t.Fatalf("read an excluded gap at %#x", at)
				}
				return 0, nil, false
			}
			layout, err := readLuaRootImageLayout(bytes.NewReader(f.diskHeader), rootRecipeUnknownHash(), f.size)
			if err != nil {
				t.Fatal(err)
			}
			layout.readable = []Range{{Start: start, End: start + 4096}}
			binding, err := ResolveLuaMailboxRoot(context.Background(), f.base, f.size, rootRecipeUnknownHash(), layout, f.read)
			if err != nil || binding.RootRVA != tc.root || binding.AnchorRVA != tc.anchor || binding.Method != "recipe_readable_text" {
				t.Fatal(binding, err)
			}
			e := binding.Evidence
			if e.ScanComplete || e.ScannedBytes != 4096 || e.TextBytes != 0x4800000-0x1000 || len(e.Gaps) != 2 || e.PatternMatches != 1 || e.RootCandidates != 1 {
				t.Fatal(e)
			}
		})
	}
}

func TestLuaRootScanPreservesShortReadPrefixAndNeverBridgesGap(t *testing.T) {
	for _, acrossGap := range []bool{false, true} {
		f := newRootRecipeFixture(t, 0x2000, 3*4096, 0x6000)
		root := f.dataRVA + 16
		f.foreverAnchor(t, f.textRVA+128, root)
		f.foreverAnchor(t, f.textRVA+8192+128, root)
		if acrossGap {
			code := bytes.Clone(f.text[128 : 128+49])
			clear(f.text)
			copy(f.text[4096-20:4096], code[:20])
			copy(f.text[8192:], code[20:])
		}
		f.hook = func(at uint64, b []byte) (int, error, bool) {
			if at == f.textRVA && len(b) > 4096 {
				return copy(b[:4096], f.text[:4096]), io.ErrUnexpectedEOF, true
			}
			if at >= f.textRVA+4096 && at < f.textRVA+8192 {
				return 0, io.EOF, true
			}
			return 0, nil, false
		}
		binding, err := f.resolve(context.Background(), rootRecipeUnknownHash())
		e := binding.Evidence
		if acrossGap {
			var scanErr *LuaRootScanError
			if !errors.As(err, &scanErr) {
				t.Fatal("matched across unreadable bytes", binding, err)
			}
			e = scanErr.Evidence
		} else if err != nil || binding.RootRVA != root || e.PatternMatches != 2 || e.RootCandidates != 1 {
			t.Fatal(binding, err)
		}
		if e.ScannedBytes != 8192 || e.ScanComplete || len(e.Gaps) != 1 || e.Gaps[0].Start != f.textRVA+4096 || e.Gaps[0].End != f.textRVA+8192 || e.ReadFailures != 2 {
			t.Fatal(e)
		}
	}
}

func TestLuaRootScanCrossesAdjacentReadableRegionsAndRecoversZeroRead(t *testing.T) {
	for _, zero := range []bool{false, true} {
		f := newRootRecipeFixture(t, 0x2000, 8192, 0x5000)
		f.foreverAnchor(t, f.textRVA+4096-20, f.dataRVA+16)
		layout, _ := readLuaRootImageLayout(bytes.NewReader(f.diskHeader), rootRecipeUnknownHash(), f.size)
		if zero {
			f.hook = func(at uint64, b []byte) (int, error, bool) { return 0, io.EOF, at == f.textRVA && len(b) > 4096 }
		} else {
			layout.readable = []Range{{Start: f.textRVA, End: f.textRVA + 4096}, {Start: f.textRVA + 4096, End: f.textRVA + 8192}}
		}
		b, err := ResolveLuaMailboxRoot(context.Background(), f.base, f.size, rootRecipeUnknownHash(), layout, f.read)
		if err != nil || !b.Evidence.ScanComplete || b.Evidence.ScannedBytes != 8192 || b.Evidence.PatternMatches != 1 {
			t.Fatal(b, err)
		}
	}
}

func TestLuaMailboxValidatesDistinctRIPRootsBeforeSelecting(t *testing.T) {
	for _, secondValid := range []bool{false, true} {
		image := newRootRecipeFixture(t, 0x2000, 4096, 0x4000)
		first, second := image.dataRVA+16, image.dataRVA+24
		image.foreverAnchor(t, image.textRVA+64, first)
		image.foreverAnchor(t, image.textRVA+192, second)
		// A syntactic hit with an invalid RIP target cannot hide the good roots.
		image.foreverAnchor(t, image.textRVA+320, image.textRVA+16)
		heap := newMailboxFixture(t, image.base, 0x300000000)
		binary.LittleEndian.PutUint64(image.data[second-image.dataRVA:], heap.state)
		if secondValid {
			binary.LittleEndian.PutUint64(image.data[first-image.dataRVA:], heap.state)
		}
		layout, _ := readLuaRootImageLayout(bytes.NewReader(image.diskHeader), rootRecipeUnknownHash(), image.size)
		r, err := OpenLuaMailbox(context.Background(), heap, image.base, image.size, rootRecipeUnknownHash(), "fixture-release", layout, image.read)
		if secondValid {
			if !errors.Is(err, ErrLuaMailboxRootUnsupported) || r != nil {
				t.Fatal("selected one of two valid roots", err)
			}
			continue
		}
		if err != nil || r.Binding().RootRVA != second || r.Binding().Evidence.RootCandidates != 2 || r.Binding().Evidence.RejectedMatches != 1 {
			t.Fatal(r, err)
		}
		found, err := r.Lookup(context.Background(), heap.selectors["input"])
		if err != nil || len(found.Records) != 1 || heap.regions != 0 {
			t.Fatal(found, err)
		}
	}
}
