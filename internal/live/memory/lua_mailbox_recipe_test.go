package memory

import (
	"bytes"
	"context"
	"crypto/sha256"
	"debug/pe"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"testing"
)

type rootRecipeFixture struct {
	base, size, textRVA, dataRVA uint64
	header, text, data           []byte
	diskHeader                   []byte
	calls                        []struct {
		rva uint64
		n   int
	}
	hook func(uint64, []byte) (int, error, bool)
}

func newRootRecipeFixture(t *testing.T, textRVA, textSize, dataRVA uint64) *rootRecipeFixture {
	t.Helper()
	f := &rootRecipeFixture{base: 0x140000000, size: dataRVA + 4096, textRVA: textRVA, dataRVA: dataRVA, header: make([]byte, 4096), text: make([]byte, textSize), data: make([]byte, 4096)}
	if textRVA+textSize > f.size {
		f.size = textRVA + textSize
	}
	copy(f.header, "MZ")
	binary.LittleEndian.PutUint32(f.header[0x3c:], 0x80)
	copy(f.header[0x80:], "PE\x00\x00")
	write := func(at int, value any) {
		var b bytes.Buffer
		if err := binary.Write(&b, binary.LittleEndian, value); err != nil {
			t.Fatal(err)
		}
		copy(f.header[at:], b.Bytes())
	}
	write(0x84, pe.FileHeader{Machine: pe.IMAGE_FILE_MACHINE_AMD64, NumberOfSections: 2, SizeOfOptionalHeader: uint16(binary.Size(pe.OptionalHeader64{}))})
	write(0x98, pe.OptionalHeader64{Magic: 0x20b, ImageBase: f.base, SectionAlignment: 4096, FileAlignment: 512, SizeOfImage: uint32(f.size), SizeOfHeaders: 4096, NumberOfRvaAndSizes: 16})
	sections := 0x98 + binary.Size(pe.OptionalHeader64{})
	write(sections, pe.SectionHeader32{Name: [8]byte{'.', 't', 'e', 'x', 't'}, VirtualSize: uint32(textSize), VirtualAddress: uint32(textRVA), SizeOfRawData: uint32(textSize), PointerToRawData: 0x400, Characteristics: pe.IMAGE_SCN_MEM_EXECUTE | pe.IMAGE_SCN_MEM_READ | pe.IMAGE_SCN_CNT_CODE})
	write(sections+40, pe.SectionHeader32{Name: [8]byte{'.', 'd', 'a', 't', 'a'}, VirtualSize: 4096, VirtualAddress: uint32(dataRVA), SizeOfRawData: 4096, PointerToRawData: 0x600, Characteristics: pe.IMAGE_SCN_MEM_READ | pe.IMAGE_SCN_MEM_WRITE | pe.IMAGE_SCN_CNT_INITIALIZED_DATA})
	f.diskHeader = append([]byte(nil), f.header...)
	return f
}

func (f *rootRecipeFixture) resolve(ctx context.Context, hash string) (LuaRootBinding, error) {
	layout, err := readLuaRootImageLayout(bytes.NewReader(f.diskHeader), hash, f.size)
	if err != nil {
		return LuaRootBinding{}, err
	}
	return ResolveLuaMailboxRoot(ctx, f.base, f.size, hash, layout, f.read)
}
func (f *rootRecipeFixture) read(ctx context.Context, address uint64, b []byte) (int, error) {
	if err := ctx.Err(); err != nil {
		return 0, err
	}
	if address < f.base {
		return 0, errors.New("outside module")
	}
	rva := address - f.base
	f.calls = append(f.calls, struct {
		rva uint64
		n   int
	}{rva, len(b)})
	if len(b) > 1<<20 {
		return 0, errors.New("oversize module call")
	}
	if f.hook != nil {
		if n, err, handled := f.hook(rva, b); handled {
			return n, err
		}
	}
	for _, span := range []struct {
		at   uint64
		data []byte
	}{{0, f.header}, {f.textRVA, f.text}, {f.dataRVA, f.data}} {
		if rva >= span.at && rva-span.at <= uint64(len(span.data)) && uint64(len(b)) <= uint64(len(span.data))-(rva-span.at) {
			return copy(b, span.data[rva-span.at:]), nil
		}
	}
	return 0, errors.New("module gap")
}
func (f *rootRecipeFixture) anchor(t *testing.T, at, root uint64) {
	t.Helper()
	pattern, _ := luaRootPattern()
	if at < f.textRVA || at+uint64(len(pattern)) > f.textRVA+uint64(len(f.text)) {
		t.Fatal("fixture anchor range")
	}
	delta := int64(root) - int64(at) - 7
	if delta < -(1<<31) || delta > 1<<31-1 {
		t.Fatal("fixture displacement")
	}
	binary.LittleEndian.PutUint32(pattern[3:7], uint32(int32(delta)))
	for i, op := range []struct{ at, disp, length int }{{7, 3, 7}, {17, 3, 7}, {32, 2, 6}, {41, 2, 6}, {49, 2, 7}} {
		target := f.dataRVA + uint64(64+i*8)
		binary.LittleEndian.PutUint32(pattern[op.at+op.disp:], uint32(int32(int64(target)-int64(at)-int64(op.at+op.length))))
	}
	binary.LittleEndian.PutUint32(pattern[28:32], uint32(int32(int64(f.textRVA+32)-int64(at)-32)))
	copy(f.text[at-f.textRVA:], pattern)
}
func rootRecipeUnknownHash() string { return strings.Repeat("a", 64) }

func (f *rootRecipeFixture) foreverAnchor(t *testing.T, at, root uint64) {
	t.Helper()
	code, _ := parseLuaRootPattern(ForeverLuaMailboxRootPattern)
	for _, operand := range foreverLuaRootOperands {
		target := root
		if operand.call {
			target = f.textRVA + 32
		}
		delta := int64(target) - int64(at) - int64(operand.offset+operand.length)
		binary.LittleEndian.PutUint32(code[operand.offset+operand.disp:], uint32(int32(delta)))
	}
	copy(f.text[at-f.textRVA:], code)
}

func TestLuaMailboxForeverRecipeUnknownMigrationAndCrossTemplateAmbiguity(t *testing.T) {
	for _, mode := range []string{"ordinary", "chunk_crossing", "tail_duplicate", "both_templates", "two_forever", "call_data", "wrong_repeat_call", "root_readonly", "layout_byte"} {
		t.Run(mode, func(t *testing.T) {
			f := newRootRecipeFixture(t, 0x2000, 2<<20, 0x203000)
			at, root := f.textRVA+128, f.dataRVA+16
			if mode == "chunk_crossing" {
				at = f.textRVA + (1 << 20) - 20
			}
			if mode == "tail_duplicate" {
				at = f.textRVA + (1 << 20) - 52
			}
			f.foreverAnchor(t, at, root)
			switch mode {
			case "both_templates":
				f.anchor(t, at+256, root)
			case "two_forever":
				f.foreverAnchor(t, at+256, root)
			case "call_data":
				binary.LittleEndian.PutUint32(f.text[at-f.textRVA+11:], uint32(int32(int64(root)-int64(at)-15)))
			case "wrong_repeat_call":
				binary.LittleEndian.PutUint32(f.text[at-f.textRVA+22:], uint32(int32(int64(f.textRVA+40)-int64(at)-26)))
			case "root_readonly":
				sectionAt := 0x98 + binary.Size(pe.OptionalHeader64{})
				binary.LittleEndian.PutUint32(f.header[sectionAt+40+36:], pe.IMAGE_SCN_MEM_READ)
				f.diskHeader = append([]byte(nil), f.header...)
			case "layout_byte":
				f.text[at-f.textRVA+35] = 9
			}
			binding, err := f.resolve(context.Background(), rootRecipeUnknownHash())
			if mode == "ordinary" || mode == "chunk_crossing" || mode == "tail_duplicate" {
				if err != nil || binding.RootRVA != root || binding.AnchorRVA != at || binding.Evidence.RecipeID != ForeverLuaMailboxRootRecipeID || binding.Evidence.PatternBytes != 49 || binding.Evidence.ScannedBytes != uint64(len(f.text)) {
					t.Fatal(binding, err)
				}
			} else if !errors.Is(err, ErrLuaMailboxRootUnsupported) {
				t.Fatalf("unsafe %s accepted: %+v %v", mode, binding, err)
			}
		})
	}
}

func TestLuaMailboxForeverExactBuildGuardsAvoidProtectedText(t *testing.T) {
	for _, mode := range []string{"valid", "raw_bytes_changed", "metadata_changed", "root_changed", "anchor_reread_changed", "module_read_denied", "cancel"} {
		t.Run(mode, func(t *testing.T) {
			// Only the bounded known site is readable. Metadata describes the real
			// large code range; no synthetic full-text allocation or scan is needed.
			f := newRootRecipeFixture(t, 0x1000, 4096, ForeverLuaMailboxRootRVA&^4095)
			sectionAt := 0x98 + binary.Size(pe.OptionalHeader64{})
			binary.LittleEndian.PutUint32(f.header[sectionAt+8:], 0x4800000-0x1000)
			f.diskHeader = append([]byte(nil), f.header...)
			code, _ := hex.DecodeString(foreverLuaMailboxAnchorHex)
			if mode == "raw_bytes_changed" {
				code[3] ^= 8
			}
			if mode == "metadata_changed" {
				f.header[sectionAt+36] ^= 1
			}
			ctx, cancel := context.WithCancel(context.Background())
			defer cancel()
			anchors, roots := 0, 0
			f.hook = func(rva uint64, b []byte) (int, error, bool) {
				if rva == ForeverLuaMailboxKnownAnchorRVA {
					anchors++
					if mode == "module_read_denied" {
						return 0, errors.New("NOACCESS"), true
					}
					copy(b, code)
					if mode == "anchor_reread_changed" && anchors > 1 {
						b[0] ^= 1
					}
					if mode == "cancel" {
						cancel()
					}
					return len(b), nil, true
				}
				if rva == ForeverLuaMailboxRootRVA && mode == "root_changed" {
					roots++
					b[0] = byte(roots)
					return len(b), nil, true
				}
				if rva >= 0x1000 && rva < 0x4800000 {
					t.Errorf("known build scanned protected text at %#x", rva)
					return 0, io.EOF, true
				}
				return 0, nil, false
			}
			binding, err := f.resolve(ctx, ForeverLuaMailboxExecutableSHA256)
			if mode == "valid" {
				if err != nil || binding.RootRVA != ForeverLuaMailboxRootRVA || binding.AnchorRVA != ForeverLuaMailboxKnownAnchorRVA || binding.Method != "known_anchor" || binding.Evidence.RecipeID != ForeverLuaMailboxRootRecipeID || binding.Evidence.ScannedBytes != 0 || !binding.Evidence.StableRootGuard {
					t.Fatal(binding, err)
				}
			} else if err == nil {
				t.Fatalf("%s silently bound", mode)
			}
			if mode == "cancel" && !errors.Is(err, context.Canceled) {
				t.Fatal("cancellation discarded", err)
			}
		})
	}
}

func TestLuaMailboxRootRecipeRuntimePEAndASLR(t *testing.T) {
	for _, mode := range []string{"unknown", "aslr", "chunk_boundary", "negative_displacement", "null_root"} {
		t.Run(mode, func(t *testing.T) {
			f := newRootRecipeFixture(t, 0x2000, 2<<20, 0x203000)
			anchor, root := f.textRVA+128, f.dataRVA+16
			if mode == "aslr" {
				f.base = 0x7ff600000000
			}
			if mode == "chunk_boundary" {
				anchor = f.textRVA + (1 << 20) - 17
			}
			if mode == "negative_displacement" {
				f = newRootRecipeFixture(t, 0x3000, 4096, 0x1000)
				anchor, root = 0x3100, 0x1010
			}
			if mode != "null_root" {
				binary.LittleEndian.PutUint64(f.data[root-f.dataRVA:], 0x12345678)
			}
			f.anchor(t, anchor, root)
			binding, err := f.resolve(context.Background(), rootRecipeUnknownHash())
			if err != nil || binding.RootRVA != root || binding.AnchorRVA != anchor || binding.Method != "recipe_unique_text" || !binding.validFor(f.base, f.size, rootRecipeUnknownHash()) {
				t.Fatal(binding, err)
			}
			if binding.Evidence.ScannedBytes != uint64(len(f.text)) || !binding.Evidence.StableRootGuard || binding.Evidence.PatternBytes != len(strings.Fields(LuaMailboxRootPattern)) {
				t.Fatal(binding.Evidence)
			}
			for _, call := range f.calls {
				if call.n > 1<<20 {
					t.Fatal("oversize read", call)
				}
			}
		})
	}
}
func TestLuaMailboxRootRecipeKnownAnchorAndMovedFallback(t *testing.T) {
	for _, mode := range []string{"known", "moved", "masked_displacement"} {
		t.Run(mode, func(t *testing.T) {
			f := newRootRecipeFixture(t, 0x600000, 1<<20, RetailLuaMailboxRootRVA&^4095)
			anchor, root := LuaMailboxKnownAnchorRVA, RetailLuaMailboxRootRVA
			if mode == "moved" {
				anchor += 256
			}
			if mode == "masked_displacement" {
				root += 8
			}
			f.anchor(t, anchor, root)
			binding, err := f.resolve(context.Background(), RetailLuaMailboxExecutableSHA256)
			if err != nil || binding.RootRVA != root || binding.AnchorRVA != anchor {
				t.Fatal(binding, err)
			}
			if mode == "known" {
				if binding.Method != "known_anchor" || binding.Evidence.ScannedBytes != 0 || binding.Evidence.ModuleReadBytes > 1<<16 {
					t.Fatal("known anchor scanned code", binding.Evidence)
				}
			} else if binding.Method != "recipe_unique_text" || binding.Evidence.ScannedBytes != uint64(len(f.text)) {
				t.Fatal(binding)
			}
			forged := binding
			forged.RootRVA += 8
			if forged.validFor(f.base, f.size, RetailLuaMailboxExecutableSHA256) || (LuaRootBinding{RootRVA: root, AnchorRVA: anchor, ExecutableSHA256: RetailLuaMailboxExecutableSHA256, Method: binding.Method}).validFor(f.base, f.size, RetailLuaMailboxExecutableSHA256) {
				t.Fatal("unsealed binding accepted")
			}
			if binding.validFor(f.base+4096, f.size, RetailLuaMailboxExecutableSHA256) {
				t.Fatal("binding moved without validation")
			}
		})
	}
}
func TestLuaMailboxRootRecipeRejectsIncompleteOrUnsafeImages(t *testing.T) {
	for _, mode := range []string{"missing", "ambiguous", "instruction_changed", "root_exec", "root_unaligned", "root_outside", "root_negative", "root_readonly", "text_nonexec", "multiple_text", "text_limit", "section_overlap", "short_text", "short_header", "read_gap", "anchor_changed", "root_changed", "unsigned_overflow", "header_budget", "header_changed", "globals_exec", "call_data", "cancel"} {
		t.Run(mode, func(t *testing.T) {
			f := newRootRecipeFixture(t, 0x2000, 8192, 0x5000)
			anchor, root := uint64(0x2100), uint64(0x5010)
			f.anchor(t, anchor, root)
			sectionAt := 0x98 + binary.Size(pe.OptionalHeader64{})
			ctx, cancel := context.WithCancel(context.Background())
			defer cancel()
			switch mode {
			case "missing":
				clear(f.text)
			case "ambiguous":
				f.anchor(t, anchor+512, root)
			case "instruction_changed":
				f.text[anchor-f.textRVA+16] ^= 1
			case "root_exec":
				f.anchor(t, anchor, 0x2200)
			case "root_unaligned":
				f.anchor(t, anchor, root+1)
			case "root_outside":
				f.anchor(t, anchor, f.size+16)
			case "root_negative":
				binary.LittleEndian.PutUint32(f.text[anchor-f.textRVA+3:], uint32(0x80000000))
			case "root_readonly":
				binary.LittleEndian.PutUint32(f.header[sectionAt+40+36:], pe.IMAGE_SCN_MEM_READ)
			case "text_nonexec":
				binary.LittleEndian.PutUint32(f.header[sectionAt+36:], pe.IMAGE_SCN_MEM_READ)
			case "multiple_text":
				copy(f.header[sectionAt+40:], []byte(".text\x00\x00\x00"))
			case "text_limit":
				binary.LittleEndian.PutUint32(f.header[sectionAt+8:], 128<<20+1)
				binary.LittleEndian.PutUint32(f.header[0x98+56:], 256<<20)
				f.size = 256 << 20
			case "section_overlap":
				binary.LittleEndian.PutUint32(f.header[sectionAt+40+12:], 0x3000)
			case "short_text":
				f.hook = func(rva uint64, b []byte) (int, error, bool) { return len(b) - 1, nil, rva == f.textRVA }
			case "short_header":
				f.hook = func(rva uint64, b []byte) (int, error, bool) { return len(b) - 1, nil, rva == 0 }
			case "read_gap":
				f.hook = func(rva uint64, b []byte) (int, error, bool) { return 0, io.EOF, rva == f.textRVA }
			case "anchor_changed":
				f.hook = func(rva uint64, b []byte) (int, error, bool) {
					if rva == anchor {
						copy(b, f.text[anchor-f.textRVA:])
						b[0] ^= 1
						return len(b), nil, true
					}
					return 0, nil, false
				}
			case "root_changed":
				reads := 0
				f.hook = func(rva uint64, b []byte) (int, error, bool) {
					if rva == root {
						reads++
						b[0] = byte(reads)
						return len(b), nil, true
					}
					return 0, nil, false
				}
			case "unsigned_overflow":
				f.base = ^uint64(0) - f.size + 1
			case "header_budget":
				binary.LittleEndian.PutUint32(f.header[0x3c:], 2<<20)
			case "header_changed":
				f.hook = func(rva uint64, b []byte) (int, error, bool) {
					if rva == f.textRVA {
						f.header[sectionAt+12] ^= 1
					}
					return 0, nil, false
				}
			case "globals_exec":
				binary.LittleEndian.PutUint32(f.text[anchor-f.textRVA+10:], uint32(int32(int64(f.textRVA+32)-int64(anchor)-14)))
			case "call_data":
				binary.LittleEndian.PutUint32(f.text[anchor-f.textRVA+28:], uint32(int32(int64(root)-int64(anchor)-32)))
			case "cancel":
				f.hook = func(rva uint64, b []byte) (int, error, bool) {
					if rva == f.textRVA {
						cancel()
					}
					return 0, nil, false
				}
			}
			binding, err := f.resolve(ctx, rootRecipeUnknownHash())
			if err == nil || binding.proof != nil {
				t.Fatal("unsafe root accepted", binding, err)
			}
			if mode == "cancel" {
				if !errors.Is(err, context.Canceled) {
					t.Fatal(err)
				}
			} else if !errors.Is(err, ErrLuaMailboxRootUnsupported) {
				t.Fatal("unsupported classification lost", err)
			}
			if mode == "unsigned_overflow" && len(f.calls) != 0 {
				t.Fatal("invalid module read")
			}
		})
	}
}

// Optional evidence replay uses untouched runtime section bytes. Headers are
// synthetic PE metadata from the checked dump manifest; this is not a live read
// or an assertion that the original runtime PE headers were captured.
func TestLuaMailboxRootRecipeOriginalRuntimeDump(t *testing.T) {
	manifestPath := os.Getenv("LYCHEEDEV_LUA_ROOT_REPLAY_MANIFEST")
	if manifestPath == "" {
		t.Skip("set LYCHEEDEV_LUA_ROOT_REPLAY_MANIFEST to existing runtime dump manifest")
	}
	raw, err := os.ReadFile(manifestPath)
	if err != nil {
		t.Fatal(err)
	}
	if len(raw) > 1<<20 {
		t.Fatal("manifest limit")
	}
	var manifest struct {
		Complete bool   `json:"complete"`
		Hash     string `json:"executableSha256"`
		Size     uint64 `json:"moduleSize"`
		Sections []struct {
			Name, RVA, SHA256 string
			VirtualSize       uint32            `json:"virtualSize"`
			ShortRead         bool              `json:"shortRead"`
			Gaps              []json.RawMessage `json:"gaps"`
		}
	}
	if err = json.Unmarshal(raw, &manifest); err != nil || !manifest.Complete || manifest.Hash != RetailLuaMailboxExecutableSHA256 {
		t.Fatal("unsupported replay manifest", err)
	}
	f := newRootRecipeFixture(t, 0x1000, 4096, RetailLuaMailboxRootRVA&^4095)
	sectionAt := 0x98 + binary.Size(pe.OptionalHeader64{})
	found := 0
	for _, section := range manifest.Sections {
		index := 0
		if section.Name == ".data" {
			index = 1
		} else if section.Name != ".text" {
			continue
		}
		if section.ShortRead || len(section.Gaps) != 0 {
			t.Fatal("incomplete original runtime section", section.Name)
		}
		at, err := strconv.ParseUint(section.RVA, 0, 64)
		if err != nil {
			t.Fatal(err)
		}
		contents, err := os.ReadFile(filepath.Join(filepath.Dir(manifestPath), section.Name+".bin"))
		if err != nil {
			t.Fatal(err)
		}
		hash := sha256.Sum256(contents)
		if uint64(len(contents)) != uint64(section.VirtualSize) || hex.EncodeToString(hash[:]) != section.SHA256 {
			t.Fatal("original section integrity", section.Name)
		}
		binary.LittleEndian.PutUint32(f.header[sectionAt+index*40+8:], section.VirtualSize)
		binary.LittleEndian.PutUint32(f.header[sectionAt+index*40+12:], uint32(at))
		if index == 0 {
			f.textRVA, f.text = at, contents
		} else {
			f.dataRVA, f.data = at, contents
		}
		found++
	}
	if found != 2 {
		t.Fatal("missing replay sections")
	}
	f.size = manifest.Size
	binary.LittleEndian.PutUint32(f.header[0x98+56:], uint32(f.size))
	f.diskHeader = append([]byte(nil), f.header...)
	for _, hash := range []string{manifest.Hash, rootRecipeUnknownHash()} {
		binding, err := f.resolve(context.Background(), hash)
		if err != nil || binding.RootRVA != RetailLuaMailboxRootRVA || binding.AnchorRVA != LuaMailboxKnownAnchorRVA {
			t.Fatal("runtime recipe replay", binding, err)
		}
		if hash == manifest.Hash && binding.Evidence.ScannedBytes != 0 {
			t.Fatal("known runtime fastpath scanned")
		}
		if hash != manifest.Hash && binding.Evidence.ScannedBytes != uint64(len(f.text)) {
			t.Fatal("recipe did not cover original text")
		}
	}
}

func TestLuaMailboxRootRecipeUsesTrustedLayoutForProtectedRuntimeHeader(t *testing.T) {
	for _, hash := range []string{RetailLuaMailboxExecutableSHA256, rootRecipeUnknownHash()} {
		f := newRootRecipeFixture(t, 0x600000, 1<<20, RetailLuaMailboxRootRVA&^4095)
		f.anchor(t, LuaMailboxKnownAnchorRVA, RetailLuaMailboxRootRVA)
		// Captured runtime COFF machine was 0x200; unrelated optional fields
		// differ too. None grants section bounds. Locating metadata stays exact.
		binary.LittleEndian.PutUint16(f.header[0x84:], 0x200)
		binary.LittleEndian.PutUint32(f.header[0x98+4:], 0xffffffff)
		binary.LittleEndian.PutUint32(f.header[0x98+56:], 0x12345678)
		unchanged := append([]byte(nil), f.header...)
		binding, err := f.resolve(context.Background(), hash)
		if err != nil || binding.RootRVA != RetailLuaMailboxRootRVA {
			t.Fatal(binding, err)
		}
		if !bytes.Equal(unchanged, f.header) {
			t.Fatal("runtime header was normalized or patched")
		}
	}
}

func TestLuaMailboxRootRecipeRequiresIssuedLayoutAndExactRuntimeMetadata(t *testing.T) {
	for _, mode := range []string{"missing_layout", "wrong_hash", "wrong_size", "mz", "lfanew", "signature", "section_count", "optional_size", "optional_magic", "section_table"} {
		t.Run(mode, func(t *testing.T) {
			f := newRootRecipeFixture(t, 0x2000, 8192, 0x5000)
			f.anchor(t, 0x2100, 0x5010)
			hash := rootRecipeUnknownHash()
			layout, err := readLuaRootImageLayout(bytes.NewReader(f.diskHeader), hash, f.size)
			if err != nil {
				t.Fatal(err)
			}
			size := f.size
			switch mode {
			case "missing_layout":
				layout = LuaRootImageLayout{}
			case "wrong_hash":
				hash = strings.Repeat("b", 64)
			case "wrong_size":
				size += 4096
			case "mz":
				f.header[0] ^= 1
			case "lfanew":
				f.header[0x3c] ^= 1
			case "signature":
				f.header[0x80] ^= 1
			case "section_count":
				f.header[0x86] ^= 1
			case "optional_size":
				f.header[0x94] ^= 1
			case "optional_magic":
				f.header[0x98] ^= 1
			case "section_table":
				f.header[0x98+binary.Size(pe.OptionalHeader64{})+36] ^= 1
			}
			binding, err := ResolveLuaMailboxRoot(context.Background(), f.base, size, hash, layout, f.read)
			if binding.proof != nil || !errors.Is(err, ErrLuaMailboxRootUnsupported) {
				t.Fatal(binding, err)
			}
			if mode == "missing_layout" || mode == "wrong_hash" || mode == "wrong_size" {
				if len(f.calls) != 0 {
					t.Fatal("invalid layout performed runtime read")
				}
			}
		})
	}
}

// The raw current header capture is pid_only evidence. This replay neither
// upgrades its identity assurance nor performs a process/game read. The source
// executable digest and all original runtime .text/.data bytes are checked.
func TestLuaMailboxRootRecipeCapturedRuntimeHeader(t *testing.T) {
	headerPath := os.Getenv("LYCHEEDEV_LUA_ROOT_RUNTIME_HEADER")
	executable := os.Getenv("LYCHEEDEV_LUA_ROOT_EXECUTABLE")
	manifestPath := os.Getenv("LYCHEEDEV_LUA_ROOT_REPLAY_MANIFEST")
	if headerPath == "" || executable == "" || manifestPath == "" {
		t.Skip("set captured runtime header, original dump manifest and hashed executable for offline replay")
	}
	raw, err := os.ReadFile(headerPath)
	if err != nil {
		t.Fatal(err)
	}
	var capture struct {
		Complete                   bool
		BytesRead                  int
		DataHex, IdentityAssurance string
	}
	if err = json.Unmarshal(raw, &capture); err != nil || !capture.Complete || capture.BytesRead != 4096 || capture.IdentityAssurance != "pid_only" {
		t.Fatal("raw header capture assurance", err)
	}
	header, err := hex.DecodeString(capture.DataHex)
	if err != nil || len(header) != 4096 {
		t.Fatal("captured header size", err)
	}
	file, err := os.Open(executable)
	if err != nil {
		t.Fatal(err)
	}
	defer file.Close()
	sum := sha256.New()
	if _, err = io.Copy(sum, file); err != nil {
		t.Fatal(err)
	}
	if hex.EncodeToString(sum.Sum(nil)) != RetailLuaMailboxExecutableSHA256 {
		t.Fatal("executable digest changed")
	}
	raw, err = os.ReadFile(manifestPath)
	if err != nil {
		t.Fatal(err)
	}
	var manifest struct {
		Complete bool
		Hash     string `json:"executableSha256"`
		Size     uint64 `json:"moduleSize"`
		Sections []struct {
			Name, RVA, SHA256 string
			VirtualSize       uint32
			ShortRead         bool
			Gaps              []json.RawMessage
		}
	}
	if err = json.Unmarshal(raw, &manifest); err != nil || !manifest.Complete || manifest.Hash != RetailLuaMailboxExecutableSHA256 {
		t.Fatal("replay manifest", err)
	}
	layout, err := readLuaRootImageLayout(file, manifest.Hash, manifest.Size)
	if err != nil {
		t.Fatal("real executable PE", err)
	}
	f := &rootRecipeFixture{base: 0x140000000, size: manifest.Size, header: header}
	found := 0
	for _, section := range manifest.Sections {
		if section.Name != ".text" && section.Name != ".data" {
			continue
		}
		if section.ShortRead || len(section.Gaps) > 0 {
			t.Fatal("incomplete section")
		}
		at, err := strconv.ParseUint(section.RVA, 0, 64)
		if err != nil {
			t.Fatal(err)
		}
		data, err := os.ReadFile(filepath.Join(filepath.Dir(manifestPath), section.Name+".bin"))
		if err != nil {
			t.Fatal(err)
		}
		hash := sha256.Sum256(data)
		if len(data) != int(section.VirtualSize) || hex.EncodeToString(hash[:]) != section.SHA256 {
			t.Fatal("runtime section integrity")
		}
		if section.Name == ".text" {
			f.textRVA, f.text = at, data
		} else {
			f.dataRVA, f.data = at, data
		}
		found++
	}
	if found != 2 {
		t.Fatal("missing sections")
	}
	for _, hash := range []string{manifest.Hash, rootRecipeUnknownHash()} {
		candidate := layout
		if hash != manifest.Hash {
			candidate, err = readLuaRootImageLayout(file, hash, manifest.Size)
			if err != nil {
				t.Fatal(err)
			}
		}
		binding, err := ResolveLuaMailboxRoot(context.Background(), f.base, f.size, hash, candidate, f.read)
		if err != nil || binding.RootRVA != RetailLuaMailboxRootRVA || binding.AnchorRVA != LuaMailboxKnownAnchorRVA {
			t.Fatal("captured runtime header replay", binding, err)
		}
	}
}

type rootRecipeHeaderOnlyReader struct {
	data         []byte
	calls, total int
}

func (r *rootRecipeHeaderOnlyReader) ReadAt(b []byte, off int64) (int, error) {
	r.calls++
	r.total += len(b)
	if off < 0 || off > int64(len(r.data)) || int64(len(b)) > int64(len(r.data))-off {
		return 0, errors.New("unrelated symbol/raw region requested")
	}
	return copy(b, r.data[off:]), nil
}
func TestLuaRootImageLayoutIgnoresUnrelatedCOFFSymbols(t *testing.T) {
	f := newRootRecipeFixture(t, 0x2000, 8192, 0x5000)
	binary.LittleEndian.PutUint32(f.diskHeader[0x84+8:], 16<<20)
	binary.LittleEndian.PutUint32(f.diskHeader[0x84+12:], 100000)
	original := append([]byte(nil), f.diskHeader...)
	reader := &rootRecipeHeaderOnlyReader{data: f.diskHeader}
	layout, err := readLuaRootImageLayout(reader, rootRecipeUnknownHash(), f.size)
	if err != nil || !layout.validFor(rootRecipeUnknownHash(), f.size) || reader.total > 1<<20 || reader.calls > 512 {
		t.Fatal("header-only disk parsing", reader.calls, reader.total, err)
	}
	if !bytes.Equal(f.diskHeader, original) {
		t.Fatal("disk header was patched")
	}
}
func TestLuaRootImageLayoutRejectsInvalidFixedHeaders(t *testing.T) {
	for _, mode := range []string{"dos_truncated", "coff_truncated", "optional_truncated", "sections_truncated", "wrong_machine", "wrong_magic", "section_count", "optional_size", "header_budget", "bad_text", "overlap", "wrong_image_size"} {
		t.Run(mode, func(t *testing.T) {
			f := newRootRecipeFixture(t, 0x2000, 8192, 0x5000)
			table := 0x98 + binary.Size(pe.OptionalHeader64{})
			switch mode {
			case "dos_truncated":
				f.diskHeader = f.diskHeader[:63]
			case "coff_truncated":
				f.diskHeader = f.diskHeader[:0x90]
			case "optional_truncated":
				f.diskHeader = f.diskHeader[:0x100]
			case "sections_truncated":
				f.diskHeader = f.diskHeader[:table+79]
			case "wrong_machine":
				binary.LittleEndian.PutUint16(f.diskHeader[0x84:], 0x200)
			case "wrong_magic":
				binary.LittleEndian.PutUint16(f.diskHeader[0x98:], 0x10b)
			case "section_count":
				binary.LittleEndian.PutUint16(f.diskHeader[0x86:], 97)
			case "optional_size":
				binary.LittleEndian.PutUint16(f.diskHeader[0x94:], 0x20)
			case "header_budget":
				binary.LittleEndian.PutUint32(f.diskHeader[0x3c:], 2<<20)
			case "bad_text":
				binary.LittleEndian.PutUint32(f.diskHeader[table+36:], pe.IMAGE_SCN_MEM_READ)
			case "overlap":
				binary.LittleEndian.PutUint32(f.diskHeader[table+40+12:], 0x3000)
			case "wrong_image_size":
				binary.LittleEndian.PutUint32(f.diskHeader[0x98+56:], 0x7000)
			}
			reader := &rootRecipeHeaderOnlyReader{data: f.diskHeader}
			layout, err := readLuaRootImageLayout(reader, rootRecipeUnknownHash(), f.size)
			if err == nil || layout.proof != nil || !errors.Is(err, ErrLuaMailboxRootUnsupported) || reader.total > 1<<20 || reader.calls > 512 {
				t.Fatal(layout, err, reader.calls, reader.total)
			}
		})
	}
}
