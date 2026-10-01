package memory

import (
	"bytes"
	"context"
	"crypto/sha256"
	"debug/pe"
	"encoding/binary"
	"encoding/hex"
	"errors"
	"fmt"
	"io"
	"strconv"
	"strings"
)

// This runtime-code recipe binds a root candidate only. MailboxReader must still
// validate the current Lua types, tags, keys, schema and complete record path.
const LuaMailboxRootRecipeID = "retail-lua-state-root-rip-v1"
const LuaMailboxRootPattern = "48 8b 3d ?? ?? ?? ?? 48 8b 1d ?? ?? ?? ?? 48 8b cf 44 8b 05 ?? ?? ?? ?? 48 8b d3 e8 ?? ?? ?? ?? 8b 05 ?? ?? ?? ?? 83 c0 01 89 05 ?? ?? ?? ?? 74 ?? 83 3d ?? ?? ?? ?? 00 75 ??"
const LuaMailboxKnownAnchorRVA uint64 = 0x6749b4

// Independently researched from Forever's RunScript caller and Lua index
// resolver. This is a second code recipe, not a copied Retail root address.
const ForeverLuaMailboxRootRecipeID = "forever-lua-state-root-runscript-rip-v1"
const ForeverLuaMailboxRootPattern = "48 8b 1d ?? ?? ?? ?? 48 8b cb e8 ?? ?? ?? ?? 48 8b cb 4c 8b 38 e8 ?? ?? ?? ?? 49 8b d7 48 8b cb 44 8b 40 08 e8 ?? ?? ?? ?? 48 8b cb e8 ?? ?? ?? ??"
const ForeverLuaMailboxExecutableSHA256 = "3d2fbfb0a20567097fa9cedbeb55a8fed895cead6c1ff86103c89ee96f5ff58f"
const ForeverLuaMailboxKnownAnchorRVA uint64 = 0x75a69d
const ForeverLuaMailboxRootRVA uint64 = 0x7b780b8
const foreverLuaMailboxAnchorHex = "488b1d14da4107488bcbe82447fb03488bcb4c8b38e81947fb03498bd7488bcb448b4008e81a74fb03488bcbe80247fb03"
const luaRootHeaderBudget = 1 << 20
const luaRootTextBudget = 128 << 20
const luaRootReadChunk = 1 << 20

var ErrLuaMailboxRootUnsupported = errors.New("memory.lua_mailbox_root_unsupported")

type LuaRootEvidence struct {
	RecipeID        string `json:"recipeId"`
	PatternBytes    int    `json:"patternBytes"`
	AnchorSHA256    string `json:"anchorSHA256"`
	HeaderBytes     uint64 `json:"headerBytes"`
	ScannedBytes    uint64 `json:"scannedBytes"`
	ModuleReadBytes uint64 `json:"moduleReadBytes"`
	ReadCalls       uint64 `json:"readCalls"`
	StableRootGuard bool   `json:"stableRootGuard"`
}

type LuaRootBinding struct {
	RootRVA          uint64          `json:"rootRva"`
	AnchorRVA        uint64          `json:"anchorRva"`
	ExecutableSHA256 string          `json:"executableSHA256"`
	Method           string          `json:"method"`
	Evidence         LuaRootEvidence `json:"evidence"`
	proof            *luaRootBindingProof
}

type luaRootBindingProof struct {
	base, size, root, anchor uint64
	hash, method             string
}

func (b LuaRootBinding) validFor(base, size uint64, hash string) bool {
	p := b.proof
	return p != nil && p.base == base && p.size == size && p.hash == hash && b.RootRVA == p.root && b.AnchorRVA == p.anchor && b.ExecutableSHA256 == p.hash && b.Method == p.method
}

func luaRootError(reason string) error {
	return fmt.Errorf("%w: %s", ErrLuaMailboxRootUnsupported, reason)
}

type luaRootModule struct {
	ctx                            context.Context
	base, size                     uint64
	read                           func(context.Context, uint64, []byte) (int, error)
	calls, total, headers, scanned uint64
	headerGuards                   []luaRootHeaderGuard
	readCause                      error
}

type luaRootHeaderGuard struct {
	rva   uint64
	bytes []byte
}

func (r *luaRootModule) exact(rva uint64, b []byte) error {
	if err := r.ctx.Err(); err != nil {
		return err
	}
	if len(b) > luaRootReadChunk || rva > r.size || uint64(len(b)) > r.size-rva {
		return luaRootError("read_range")
	}
	// PE metadata plus at most one complete .text pass and fixed guards.
	if r.calls >= 2048 || r.total+uint64(len(b)) > 2*luaRootHeaderBudget+luaRootTextBudget+4096 {
		return luaRootError("read_budget")
	}
	r.calls++
	r.total += uint64(len(b))
	n, err := r.read(r.ctx, r.base+rva, b)
	if err != nil {
		r.readCause = errors.Join(luaRootError("module_read"), err)
		return r.readCause
	}
	if n != len(b) {
		r.readCause = errors.Join(luaRootError("short_read"), io.ErrUnexpectedEOF)
		return r.readCause
	}
	return r.ctx.Err()
}

// The fixed PE structure decoder consumes disk metadata through ReaderAt.
// Mapped runtime code is at VirtualAddress, never a disk raw-data offset.
func (r *luaRootModule) ReadAt(b []byte, off int64) (int, error) {
	if off < 0 || uint64(off) > luaRootHeaderBudget || uint64(len(b)) > luaRootHeaderBudget-uint64(off) || r.headers+uint64(len(b)) > luaRootHeaderBudget || len(r.headerGuards) >= 512 {
		return 0, luaRootError("header_budget")
	}
	r.headers += uint64(len(b))
	if err := r.exact(uint64(off), b); err != nil {
		return 0, err
	}
	r.headerGuards = append(r.headerGuards, luaRootHeaderGuard{uint64(off), append([]byte(nil), b...)})
	return len(b), nil
}

func luaRootPattern() ([]byte, []bool) {
	return parseLuaRootPattern(LuaMailboxRootPattern)
}
func parseLuaRootPattern(text string) ([]byte, []bool) {
	fields := strings.Fields(text)
	pattern, mask := make([]byte, len(fields)), make([]bool, len(fields))
	for i, field := range fields {
		if field != "??" {
			n, err := strconv.ParseUint(field, 16, 8)
			if err != nil {
				panic("invalid immutable Lua root recipe")
			}
			pattern[i] = byte(n)
			mask[i] = true
		}
	}
	return pattern, mask
}
func luaRootMatches(b, pattern []byte, mask []bool) bool {
	if len(b) != len(pattern) {
		return false
	}
	for i, v := range pattern {
		if mask[i] && b[i] != v {
			return false
		}
	}
	return true
}

type luaRootSection struct {
	start, end uint64
	flags      uint32
	text       bool
}

type luaRootOperand struct {
	offset, disp, length, width int
	call                        bool
	sameTarget                  int
}

var retailLuaRootOperands = []luaRootOperand{
	{0, 3, 7, 8, false, 0}, {7, 3, 7, 8, false, 0}, {17, 3, 7, 4, false, 0},
	{27, 1, 5, 1, true, 0}, {32, 2, 6, 4, false, 0}, {41, 2, 6, 4, false, 0}, {49, 2, 7, 4, false, 0},
}
var foreverLuaRootOperands = []luaRootOperand{
	{0, 3, 7, 8, false, 0}, {10, 1, 5, 1, true, 1}, {21, 1, 5, 1, true, 1},
	{36, 1, 5, 1, true, 0}, {44, 1, 5, 1, true, 1},
}

func luaRootTarget(anchor uint64, code []byte, size uint64, sections []luaRootSection) (uint64, error) {
	return luaRootRecipeTarget(anchor, code, size, sections, retailLuaRootOperands)
}
func luaRootRecipeTarget(anchor uint64, code []byte, size uint64, sections []luaRootSection, operands []luaRootOperand) (uint64, error) {
	// Both recipes start with a seven-byte RIP-relative state pointer load.
	if anchor > size || uint64(len(code)) > size-anchor {
		return 0, luaRootError("anchor_range")
	}
	var root uint64
	repeatedTargets := make(map[int]uint64)
	// Frozen instruction boundaries cover all RIP-relative data and call targets.
	// Displacements remain relocatable; repeated state getter calls stay identical.
	for _, operand := range operands {
		if operand.offset+operand.disp+4 > len(code) {
			return 0, luaRootError("anchor_range")
		}
		target := int64(anchor) + int64(operand.offset+operand.length) + int64(int32(binary.LittleEndian.Uint32(code[operand.offset+operand.disp:])))
		if target < 0 || uint64(target) > size || uint64(operand.width) > size-uint64(target) {
			return 0, luaRootError("rip_target_range")
		}
		at := uint64(target)
		if operand.offset == 0 {
			root = at
			if root%8 != 0 {
				return 0, luaRootError("root_alignment")
			}
		}
		valid := false
		for _, s := range sections {
			if at < s.start || at+uint64(operand.width) > s.end || s.flags&pe.IMAGE_SCN_MEM_READ == 0 {
				continue
			}
			if operand.call {
				valid = s.text && s.flags&pe.IMAGE_SCN_MEM_EXECUTE != 0
			} else {
				valid = s.flags&pe.IMAGE_SCN_MEM_WRITE != 0 && s.flags&pe.IMAGE_SCN_MEM_EXECUTE == 0
			}
			if valid {
				break
			}
		}
		if !valid {
			return 0, luaRootError("rip_target_section")
		}
		if operand.sameTarget != 0 {
			if previous, exists := repeatedTargets[operand.sameTarget]; exists && previous != at {
				return 0, luaRootError("call_target_mismatch")
			}
			repeatedTargets[operand.sameTarget] = at
		}
	}
	return root, nil
}

// LuaRootImageLayout is issued from the same locked executable handle that
// MainModule hashes. Its opaque, immutable metadata is never caller-supplied RVA
// authority. Runtime code and locating metadata are still verified separately.
type LuaRootImageLayout struct{ proof *luaRootImageLayoutProof }
type luaRootImageLayoutProof struct {
	hash                  string
	moduleSize, imageSize uint64
	sections              []luaRootSection
	text                  luaRootSection
	guards                []luaRootHeaderGuard
}

func (l LuaRootImageLayout) validFor(hash string, size uint64) bool {
	return l.proof != nil && l.proof.hash == hash && l.proof.moduleSize == size
}

// The caller must supply the already-hashed, read-locked executable handle.
// This parser has no path-opening or hash-specific layout exception.
func readLuaRootImageLayout(reader io.ReaderAt, executableSHA256 string, moduleSize uint64) (LuaRootImageLayout, error) {
	digest, err := hex.DecodeString(executableSHA256)
	if reader == nil || moduleSize == 0 || moduleSize > 1<<32 || err != nil || len(digest) != 32 || executableSHA256 != strings.ToLower(executableSHA256) {
		return LuaRootImageLayout{}, luaRootError("image_identity")
	}
	r := &luaRootModule{ctx: context.Background(), base: 1, size: luaRootHeaderBudget, read: func(_ context.Context, address uint64, b []byte) (int, error) {
		return reader.ReadAt(b, int64(address-1))
	}}
	// Root location needs no COFF symbols, string table, relocations or disk
	// code. Decode only standard fixed PE structures; never alter their bytes.
	dos := make([]byte, 64)
	if _, err := r.ReadAt(dos, 0); err != nil {
		return LuaRootImageLayout{}, err
	}
	if dos[0] != 'M' || dos[1] != 'Z' {
		return LuaRootImageLayout{}, luaRootError("dos_header")
	}
	nt := uint64(binary.LittleEndian.Uint32(dos[0x3c:]))
	signature := make([]byte, 4)
	if _, err := r.ReadAt(signature, int64(nt)); err != nil {
		return LuaRootImageLayout{}, err
	}
	if !bytes.Equal(signature, []byte{'P', 'E', 0, 0}) {
		return LuaRootImageLayout{}, luaRootError("nt_signature")
	}
	coff := make([]byte, binary.Size(pe.FileHeader{}))
	if _, err := r.ReadAt(coff, int64(nt+4)); err != nil {
		return LuaRootImageLayout{}, err
	}
	var file pe.FileHeader
	if err := binary.Read(bytes.NewReader(coff), binary.LittleEndian, &file); err != nil {
		return LuaRootImageLayout{}, errors.Join(luaRootError("file_header"), err)
	}
	if file.Machine != pe.IMAGE_FILE_MACHINE_AMD64 || file.NumberOfSections == 0 || file.NumberOfSections > 96 || int(file.SizeOfOptionalHeader) < binary.Size(pe.OptionalHeader64{}) {
		return LuaRootImageLayout{}, luaRootError("image_layout")
	}
	optBytes := make([]byte, int(file.SizeOfOptionalHeader))
	if _, err := r.ReadAt(optBytes, int64(nt+24)); err != nil {
		return LuaRootImageLayout{}, err
	}
	var optional pe.OptionalHeader64
	if err := binary.Read(bytes.NewReader(optBytes), binary.LittleEndian, &optional); err != nil {
		return LuaRootImageLayout{}, errors.Join(luaRootError("optional_header"), err)
	}
	if optional.Magic != 0x20b || optional.SizeOfImage == 0 || uint64(optional.SizeOfImage) != moduleSize || optional.SizeOfHeaders == 0 || uint64(optional.SizeOfHeaders) > moduleSize {
		return LuaRootImageLayout{}, luaRootError("image_layout")
	}
	table := nt + 24 + uint64(file.SizeOfOptionalHeader)
	tableBytes := make([]byte, int(file.NumberOfSections)*binary.Size(pe.SectionHeader32{}))
	if table+uint64(len(tableBytes)) > uint64(optional.SizeOfHeaders) {
		return LuaRootImageLayout{}, luaRootError("section_headers_range")
	}
	if _, err := r.ReadAt(tableBytes, int64(table)); err != nil {
		return LuaRootImageLayout{}, err
	}
	var rawSections []pe.SectionHeader32
	sectionReader := bytes.NewReader(tableBytes)
	for i := 0; i < int(file.NumberOfSections); i++ {
		var section pe.SectionHeader32
		if err := binary.Read(sectionReader, binary.LittleEndian, &section); err != nil {
			return LuaRootImageLayout{}, errors.Join(luaRootError("section_headers"), err)
		}
		rawSections = append(rawSections, section)
	}
	imageSize := uint64(optional.SizeOfImage)
	var sections []luaRootSection
	var text luaRootSection
	textCount := 0
	for _, s := range rawSections {
		start, length := uint64(s.VirtualAddress), uint64(s.VirtualSize)
		if length == 0 || start > imageSize || length > imageSize-start {
			return LuaRootImageLayout{}, luaRootError("section_range")
		}
		section := luaRootSection{start, start + length, s.Characteristics, s.Name == ([8]byte{'.', 't', 'e', 'x', 't'})}
		for _, prior := range sections {
			if start < prior.end && section.end > prior.start {
				return LuaRootImageLayout{}, luaRootError("section_overlap")
			}
		}
		sections = append(sections, section)
		if s.Name == ([8]byte{'.', 't', 'e', 'x', 't'}) {
			textCount++
			text = section
		}
	}
	if textCount != 1 || text.flags&pe.IMAGE_SCN_MEM_EXECUTE == 0 || text.flags&pe.IMAGE_SCN_MEM_READ == 0 || text.end-text.start > luaRootTextBudget {
		return LuaRootImageLayout{}, luaRootError("text_section")
	}
	// Guard only fields that locate/interpret the section table, then every
	// original section header byte. The runtime loader may transform unrelated
	// COFF/optional fields; they never supply our segment bounds.
	var guards []luaRootHeaderGuard
	for _, part := range []struct {
		rva uint64
		n   int
	}{{0, 2}, {0x3c, 4}, {nt, 4}, {nt + 6, 2}, {nt + 20, 2}, {nt + 24, 2}, {table, int(file.NumberOfSections) * 40}} {
		b := make([]byte, part.n)
		if _, err := r.ReadAt(b, int64(part.rva)); err != nil {
			return LuaRootImageLayout{}, err
		}
		guards = append(guards, luaRootHeaderGuard{part.rva, b})
	}
	return LuaRootImageLayout{&luaRootImageLayoutProof{executableSHA256, moduleSize, imageSize, sections, text, guards}}, nil
}

// ResolveLuaMailboxRoot reads an OS-sealed MEM_IMAGE/MEM_MAPPED main module.
// Process.ReadModule enforces mapping and process identity. A layout issued from
// the same hashed executable supplies bounds; exact runtime locating metadata
// and section table guards must match it. The digest selects a fast anchor
// validation, never an old RVA shortcut. No heap discovery or layout inference
// occurs, and unrelated protected runtime header fields are never normalized.
func ResolveLuaMailboxRoot(ctx context.Context, base, size uint64, executableSHA256 string, layout LuaRootImageLayout, readModule func(context.Context, uint64, []byte) (int, error)) (LuaRootBinding, error) {
	var empty LuaRootBinding
	digest, err := hex.DecodeString(executableSHA256)
	if base == 0 || size == 0 || base > ^uint64(0)-size || size > 1<<32 || len(digest) != 32 || err != nil || executableSHA256 != strings.ToLower(executableSHA256) || readModule == nil {
		return empty, luaRootError("module_identity")
	}
	r := &luaRootModule{ctx: ctx, base: base, size: size, read: readModule}
	if !layout.validFor(executableSHA256, size) {
		return empty, luaRootError("image_layout_required")
	}
	imageSize, sections, text := layout.proof.imageSize, layout.proof.sections, layout.proof.text
	for _, guard := range layout.proof.guards {
		current := make([]byte, len(guard.bytes))
		if err := r.exact(guard.rva, current); err != nil {
			return empty, err
		}
		r.headers += uint64(len(current))
		if !bytes.Equal(current, guard.bytes) {
			return empty, luaRootError("runtime_image_metadata")
		}
	}
	r.headerGuards = layout.proof.guards
	type recipe struct {
		id       string
		pattern  []byte
		mask     []bool
		operands []luaRootOperand
	}
	pattern, mask := luaRootPattern()
	secondPattern, secondMask := parseLuaRootPattern(ForeverLuaMailboxRootPattern)
	recipes := []recipe{{LuaMailboxRootRecipeID, pattern, mask, retailLuaRootOperands}, {ForeverLuaMailboxRootRecipeID, secondPattern, secondMask, foreverLuaRootOperands}}
	bind := func(anchor, root uint64, code []byte, method string, selectedRecipe recipe) (LuaRootBinding, error) {
		rootGuard, anchorGuard, rootAfter := make([]byte, 8), make([]byte, len(code)), make([]byte, 8)
		if err := r.exact(root, rootGuard); err != nil {
			return empty, err
		}
		if err := r.exact(anchor, anchorGuard); err != nil {
			return empty, err
		}
		if !bytes.Equal(code, anchorGuard) {
			return empty, luaRootError("anchor_changed")
		}
		for _, guard := range r.headerGuards {
			current := make([]byte, len(guard.bytes))
			if err := r.exact(guard.rva, current); err != nil {
				return empty, err
			}
			if !bytes.Equal(guard.bytes, current) {
				return empty, luaRootError("header_changed")
			}
		}
		if err := r.exact(root, rootAfter); err != nil {
			return empty, err
		}
		if !bytes.Equal(rootGuard, rootAfter) {
			return empty, luaRootError("root_changed")
		}
		if err := ctx.Err(); err != nil {
			return empty, err
		}
		hash := sha256.Sum256(code)
		return LuaRootBinding{RootRVA: root, AnchorRVA: anchor, ExecutableSHA256: executableSHA256, Method: method, Evidence: LuaRootEvidence{RecipeID: selectedRecipe.id, PatternBytes: len(selectedRecipe.pattern), AnchorSHA256: hex.EncodeToString(hash[:]), HeaderBytes: r.headers, ScannedBytes: r.scanned, ModuleReadBytes: r.total, ReadCalls: r.calls, StableRootGuard: true}, proof: &luaRootBindingProof{base, size, root, anchor, executableSHA256, method}}, nil
	}
	if executableSHA256 == ForeverLuaMailboxExecutableSHA256 {
		anchor := ForeverLuaMailboxKnownAnchorRVA
		if anchor < text.start || anchor > text.end || uint64(len(secondPattern)) > text.end-anchor {
			return empty, luaRootError("known_anchor_range")
		}
		code := make([]byte, len(secondPattern))
		if err := r.exact(anchor, code); err != nil {
			return empty, err
		}
		expected, _ := hex.DecodeString(foreverLuaMailboxAnchorHex)
		if !bytes.Equal(code, expected) {
			return empty, luaRootError("known_anchor_bytes")
		}
		root, err := luaRootRecipeTarget(anchor, code, imageSize, sections, foreverLuaRootOperands)
		if err != nil {
			return empty, err
		}
		if root != ForeverLuaMailboxRootRVA {
			return empty, luaRootError("known_root_target")
		}
		return bind(anchor, root, code, "known_anchor", recipes[1])
	}
	if executableSHA256 == RetailLuaMailboxExecutableSHA256 && LuaMailboxKnownAnchorRVA >= text.start && LuaMailboxKnownAnchorRVA <= text.end && uint64(len(pattern)) <= text.end-LuaMailboxKnownAnchorRVA {
		code := make([]byte, len(pattern))
		if err := r.exact(LuaMailboxKnownAnchorRVA, code); err != nil {
			return empty, err
		}
		if luaRootMatches(code, pattern, mask) {
			root, e := luaRootTarget(LuaMailboxKnownAnchorRVA, code, imageSize, sections)
			if e == nil && root == RetailLuaMailboxRootRVA {
				return bind(LuaMailboxKnownAnchorRVA, root, code, "known_anchor", recipes[0])
			}
		}
	}
	// Keep only a fixed tail to match instructions crossing a read boundary.
	buffer := make([]byte, luaRootReadChunk)
	var tail, selected []byte
	var anchor uint64
	var selectedRecipe recipe
	maxPattern := len(pattern)
	if len(secondPattern) > maxPattern {
		maxPattern = len(secondPattern)
	}
	matches := 0
	for at := text.start; at < text.end; {
		n := uint64(len(buffer))
		if n > text.end-at {
			n = text.end - at
		}
		chunk := buffer[:int(n)]
		if err := r.exact(at, chunk); err != nil {
			return empty, err
		}
		r.scanned += n
		combined := make([]byte, len(tail)+len(chunk))
		copy(combined, tail)
		copy(combined[len(tail):], chunk)
		start := at - uint64(len(tail))
		for i := 0; i < len(combined); i++ {
			if i&4095 == 0 {
				if err := ctx.Err(); err != nil {
					return empty, err
				}
			}
			for _, candidate := range recipes {
				length := len(candidate.pattern)
				if i+length > len(combined) || at > text.start && start+uint64(i+length) <= at || combined[i] != candidate.pattern[0] || !luaRootMatches(combined[i:i+length], candidate.pattern, candidate.mask) {
					continue
				}
				matches++
				if matches > 1 {
					return empty, luaRootError("ambiguous_anchor")
				}
				anchor = start + uint64(i)
				selected = append([]byte(nil), combined[i:i+length]...)
				selectedRecipe = candidate
			}
		}
		keep := maxPattern - 1
		if keep > len(combined) {
			keep = len(combined)
		}
		tail = append(tail[:0], combined[len(combined)-keep:]...)
		at += n
	}
	if matches != 1 {
		return empty, luaRootError("anchor_missing")
	}
	root, err := luaRootRecipeTarget(anchor, selected, imageSize, sections, selectedRecipe.operands)
	if err != nil {
		return empty, err
	}
	return bind(anchor, root, selected, "recipe_unique_text", selectedRecipe)
}
