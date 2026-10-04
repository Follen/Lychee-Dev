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
	"strings"
)

// Immutable archived handler manifest; no developer-machine path is required.
// Compatible new builds must locate unique instruction patterns, verify all
// current RIP targets and retain stable guards. This is not writer validation.
const RetailReloadRecipeID = "retail-ui-reload-state-rip-v1"
const retailReloadExecutableSHA256 = "d41f11de411f6fdb280a7c1c6380ba3d1b17614f03e5aca8fabce2cc715cd7dd"
const retailReloadSourceBuild = "12.1.0.69933"
const retailReloadRequestAnchor uint64 = 0x2706c8d
const retailReloadWorkerAnchor uint64 = 0x2636b55
const retailReloadWorldAnchor uint64 = 0x17bb14b
const retailReloadNormalRVA uint64 = 0x59f900d
const retailReloadModeRVA uint64 = 0x6066a38
const retailReloadRequestHex = "b907000000660fabc86689059bfd9503c3803d42232f03007410803d9a232f03007507c60556232f0301c3"
const retailReloadWorkerHex = "bb0800000048897010ba020000004c89701833c90fb705c8fea203660fabd8668905bdfea203"
const retailReloadWorldHex = "0fb605e6b88a04c0e80424014889354a8bfb0388442450"
const retailReloadRequestPattern = "b9 07 00 00 00 66 0f ab c8 66 89 05 ?? ?? ?? ?? c3 80 3d ?? ?? ?? ?? 00 74 10 80 3d ?? ?? ?? ?? 00 75 07 c6 05 ?? ?? ?? ?? 01 c3"
const retailReloadWorkerPattern = "bb 08 00 00 00 48 89 70 10 ba 02 00 00 00 4c 89 70 18 33 c9 0f b7 05 ?? ?? ?? ?? 66 0f ab d8 66 89 05 ?? ?? ?? ??"
const retailReloadWorldPattern = "0f b6 05 ?? ?? ?? ?? c0 e8 04 24 01 48 89 35 ?? ?? ?? ?? 88 44 24 50"

type ReloadRecipeEvidence struct {
	SourceBuild         string `json:"sourceBuild"`
	SourceSHA256        string `json:"sourceSHA256"`
	RequestAnchorSHA256 string `json:"requestAnchorSHA256"`
	WorkerAnchorSHA256  string `json:"workerAnchorSHA256"`
	WorldAnchorSHA256   string `json:"worldAnchorSHA256,omitempty"`
	WorldQualification  string `json:"worldQualification"`
	HeaderBytes         uint64 `json:"headerBytes"`
	ScannedBytes        uint64 `json:"scannedBytes"`
	ModuleReadBytes     uint64 `json:"moduleReadBytes"`
	ReadCalls           uint64 `json:"readCalls"`
	Validation          string `json:"validation"`
}
type ReloadResolution struct {
	RecipeID         string `json:"recipeId"`
	ExecutableSHA256 string `json:"executableSHA256"`
	Build            string `json:"build"`
	Product          string `json:"product"`
	Method           string `json:"method"`
	// NormalRVA/NormalAnchor locate the Glue pending-byte branch.
	NormalRVA         uint64               `json:"glueRequestRva"`
	ModeRVA           uint64               `json:"modeRva"`
	ModeRequestAnchor uint64               `json:"modeRequestAnchorRva"`
	NormalAnchor      uint64               `json:"glueRequestAnchorRva"`
	ModeWorkerAnchor  uint64               `json:"modeWorkerAnchorRva"`
	WorldAnchor       uint64               `json:"worldAnchorRva,omitempty"`
	Evidence          ReloadRecipeEvidence `json:"evidence"`
}

// Public fields are diagnostic projections, never caller-issued RVA authority.
type ReloadBinding struct {
	Resolution ReloadResolution `json:"resolution"`
	proof      *reloadBindingProof
}
type reloadBindingProof struct {
	base, size             uint64
	resolution             ReloadResolution
	guards                 []luaRootHeaderGuard
	request, worker, world []byte
	read                   func(context.Context, uint64, []byte) (int, error)
}

func (b *ReloadBinding) valid() bool {
	return b.proof != nil && b.Resolution == b.proof.resolution && b.proof.read != nil
}
func reloadRecipeError(reason string) error { return fmt.Errorf("%w: %s", ErrReloadUnknown, reason) }
func verifyReloadMetadata(r *luaRootModule, guards []luaRootHeaderGuard) error {
	for _, guard := range guards {
		got := make([]byte, len(guard.bytes))
		if e := r.exact(guard.rva, got); e != nil {
			return e
		}
		r.headers += uint64(len(got))
		if !bytes.Equal(got, guard.bytes) {
			return reloadRecipeError("runtime_image_metadata_changed")
		}
	}
	return nil
}
func (p *reloadBindingProof) verify(r *luaRootModule) error {
	if e := verifyReloadMetadata(r, p.guards); e != nil {
		return e
	}
	for _, anchor := range []struct {
		rva  uint64
		code []byte
	}{{p.resolution.ModeRequestAnchor, p.request}, {p.resolution.ModeWorkerAnchor, p.worker}} {
		got := make([]byte, len(anchor.code))
		if e := r.exact(anchor.rva, got); e != nil {
			return e
		}
		if !bytes.Equal(got, anchor.code) {
			return reloadRecipeError("reload_anchor_changed")
		}
	}
	return r.ctx.Err()
}
func (p *reloadBindingProof) verifyWorld(r *luaRootModule) error {
	if len(p.world) == 0 {
		return reloadRecipeError("world_getter_unqualified")
	}
	got := make([]byte, len(p.world))
	if e := r.exact(p.resolution.WorldAnchor, got); e != nil {
		return e
	}
	if !bytes.Equal(got, p.world) {
		return reloadRecipeError("world_getter_changed")
	}
	return nil
}

type reloadCandidate struct {
	anchor uint64
	code   []byte
}

// ResolveReloadState is bounded read-only discovery. The known hash/build is a
// fast path only after complete current instruction bytes and targets match.
// Other Retail builds receive a complete unique-pattern .text relocation;
// incompatible/ambiguous instructions are refused, never guessed from old RVAs.
func ResolveReloadState(ctx context.Context, base, size uint64, hash, build, product string, layout LuaRootImageLayout, readModule func(context.Context, uint64, []byte) (int, error)) (*ReloadBinding, error) {
	digest, e := hex.DecodeString(hash)
	if base == 0 || size == 0 || size > 1<<32 || base > ^uint64(0)-size || e != nil || len(digest) != 32 || hash != strings.ToLower(hash) || build == "" || product != "retail" || readModule == nil {
		return nil, reloadRecipeError("module_identity_or_product")
	}
	if !layout.validFor(hash, size) {
		return nil, reloadRecipeError("locked_image_layout_required")
	}
	r := &luaRootModule{ctx: ctx, base: base, size: size, read: readModule}
	if e = verifyReloadMetadata(r, layout.proof.guards); e != nil {
		return nil, errors.Join(ErrReloadUnknown, e)
	}
	var candidates [3]reloadCandidate
	worldQualification := "getter_not_found"
	method := "unique_text_patterns"
	if hash == retailReloadExecutableSHA256 && build == retailReloadSourceBuild {
		method = "known_anchors"
		for i, known := range []struct {
			rva uint64
			hex string
		}{{retailReloadRequestAnchor, retailReloadRequestHex}, {retailReloadWorkerAnchor, retailReloadWorkerHex}, {retailReloadWorldAnchor, retailReloadWorldHex}} {
			code, _ := hex.DecodeString(known.hex)
			if known.rva < layout.proof.text.start || known.rva > layout.proof.text.end || uint64(len(code)) > layout.proof.text.end-known.rva {
				if i == 2 {
					worldQualification = "getter_range_unqualified"
					continue
				}
				return nil, reloadRecipeError("known_anchor_range")
			}
			got := make([]byte, len(code))
			if e = r.exact(known.rva, got); e != nil {
				if i == 2 {
					worldQualification = "getter_read_unavailable"
					continue
				}
				return nil, errors.Join(ErrReloadUnknown, e)
			}
			if !bytes.Equal(got, code) {
				if i == 2 {
					worldQualification = "getter_bytes_unqualified"
					continue
				}
				return nil, reloadRecipeError("known_anchor_bytes")
			}
			candidates[i] = reloadCandidate{known.rva, got}
		}
	} else {
		var counts [3]int
		if candidates, counts, e = scanReloadPatterns(r, layout.proof.text); e != nil {
			return nil, errors.Join(ErrReloadUnknown, e)
		}
		if counts[2] > 1 {
			worldQualification = "getter_ambiguous"
		}
	}
	normal, mode, e := reloadTargets(candidates, layout.proof.imageSize, layout.proof.sections)
	if e != nil {
		return nil, e
	}
	if method == "known_anchors" && (normal != retailReloadNormalRVA || mode != retailReloadModeRVA) {
		return nil, reloadRecipeError("known_targets_changed")
	}
	worldAnchor := uint64(0)
	if len(candidates[2].code) > 0 {
		target, worldErr := reloadDataTarget(candidates[2], 3, 7, 1, layout.proof.imageSize, layout.proof.sections)
		_, auxErr := reloadDataTarget(candidates[2], 15, 19, 8, layout.proof.imageSize, layout.proof.sections)
		if worldErr == nil && auxErr == nil && target == mode {
			worldAnchor = candidates[2].anchor
			worldQualification = "getter_pattern_observed"
		} else {
			candidates[2] = reloadCandidate{}
			worldQualification = "getter_relationship_unqualified"
		}
	}
	requestHash, workerHash := sha256.Sum256(candidates[0].code), sha256.Sum256(candidates[1].code)
	resolution := ReloadResolution{RecipeID: RetailReloadRecipeID, ExecutableSHA256: hash, Build: build, Product: product, Method: method, NormalRVA: normal, ModeRVA: mode, ModeRequestAnchor: candidates[0].anchor, NormalAnchor: candidates[0].anchor + 35, ModeWorkerAnchor: candidates[1].anchor, WorldAnchor: worldAnchor}
	p := &reloadBindingProof{base: base, size: size, guards: layout.proof.guards, request: candidates[0].code, worker: candidates[1].code, world: candidates[2].code, read: readModule, resolution: resolution}
	if e = p.verify(r); e != nil {
		return nil, errors.Join(ErrReloadUnknown, e)
	}
	if len(p.world) > 0 {
		if e = p.verifyWorld(r); e != nil {
			p.world = nil
			resolution.WorldAnchor = 0
			worldQualification = "getter_changed_during_resolution"
		}
	}
	evidence := ReloadRecipeEvidence{SourceBuild: retailReloadSourceBuild, SourceSHA256: retailReloadExecutableSHA256, RequestAnchorSHA256: hex.EncodeToString(requestHash[:]), WorkerAnchorSHA256: hex.EncodeToString(workerHash[:]), WorldQualification: worldQualification, HeaderBytes: r.headers, ScannedBytes: r.scanned, ModuleReadBytes: r.total, ReadCalls: r.calls, Validation: "not_run"}
	if len(p.world) > 0 {
		worldHash := sha256.Sum256(p.world)
		evidence.WorldAnchorSHA256 = hex.EncodeToString(worldHash[:])
	}
	resolution.Evidence = evidence
	p.resolution = resolution
	return &ReloadBinding{Resolution: resolution, proof: p}, nil
}

func scanReloadPatterns(r *luaRootModule, text luaRootSection) ([3]reloadCandidate, [3]int, error) {
	var candidates [3]reloadCandidate
	var patterns [3][]byte
	var masks [3][]bool
	var counts [3]int
	maxLength := 0
	for i, recipe := range []string{retailReloadRequestPattern, retailReloadWorkerPattern, retailReloadWorldPattern} {
		patterns[i], masks[i] = parseLuaRootPattern(recipe)
		maxLength = max(maxLength, len(patterns[i]))
	}
	buffer := make([]byte, luaRootReadChunk)
	var tail []byte
	for at := text.start; at < text.end; {
		n := min(uint64(len(buffer)), text.end-at)
		chunk := buffer[:int(n)]
		if e := r.exact(at, chunk); e != nil {
			return candidates, counts, e
		}
		r.scanned += n
		combined := make([]byte, len(tail)+len(chunk))
		copy(combined, tail)
		copy(combined[len(tail):], chunk)
		start := at - uint64(len(tail))
		for offset := range combined {
			if offset&4095 == 0 {
				if e := r.ctx.Err(); e != nil {
					return candidates, counts, e
				}
			}
			for i, pattern := range patterns {
				length := len(pattern)
				if offset+length > len(combined) || at > text.start && start+uint64(offset+length) <= at || combined[offset] != pattern[0] || !luaRootMatches(combined[offset:offset+length], pattern, masks[i]) {
					continue
				}
				counts[i] = min(2, counts[i]+1)
				if counts[i] > 1 {
					if i < 2 {
						return candidates, counts, reloadRecipeError("ambiguous_reload_anchor")
					}
					candidates[i] = reloadCandidate{}
					continue
				}
				candidates[i] = reloadCandidate{start + uint64(offset), append([]byte(nil), combined[offset:offset+length]...)}
			}
		}
		keep := min(maxLength-1, len(combined))
		tail = append(tail[:0], combined[len(combined)-keep:]...)
		at += n
	}
	if counts[0] != 1 || counts[1] != 1 {
		return candidates, counts, reloadRecipeError("reload_anchor_missing")
	}
	return candidates, counts, nil
}
func reloadDataTarget(candidate reloadCandidate, displacement, next, width int, size uint64, sections []luaRootSection) (uint64, error) {
	if displacement+4 > len(candidate.code) || candidate.anchor > size || uint64(len(candidate.code)) > size-candidate.anchor {
		return 0, reloadRecipeError("anchor_range")
	}
	target := int64(candidate.anchor) + int64(next) + int64(int32(binary.LittleEndian.Uint32(candidate.code[displacement:displacement+4])))
	if target < 0 || uint64(target) > size || uint64(width) > size-uint64(target) {
		return 0, reloadRecipeError("rip_target_range")
	}
	at := uint64(target)
	for _, section := range sections {
		if at >= section.start && at+uint64(width) <= section.end && section.flags&pe.IMAGE_SCN_MEM_READ != 0 && section.flags&pe.IMAGE_SCN_MEM_WRITE != 0 && section.flags&pe.IMAGE_SCN_MEM_EXECUTE == 0 {
			return at, nil
		}
	}
	return 0, reloadRecipeError("rip_target_section_or_width")
}
func reloadTargets(candidates [3]reloadCandidate, size uint64, sections []luaRootSection) (normal, mode uint64, err error) {
	var targets [6]uint64
	for i, op := range []struct{ candidate, disp, next, width int }{{0, 12, 16, 2}, {0, 19, 24, 1}, {0, 28, 33, 1}, {0, 37, 42, 1}, {1, 23, 27, 2}, {1, 34, 38, 2}} {
		targets[i], err = reloadDataTarget(candidates[op.candidate], op.disp, op.next, op.width, size, sections)
		if err != nil {
			return 0, 0, err
		}
	}
	if targets[0] != targets[4] || targets[0] != targets[5] || targets[0]&1 != 0 {
		return 0, 0, reloadRecipeError("modeword_relationship_or_alignment")
	}
	for i := 1; i <= 3; i++ {
		if targets[i] >= targets[0] && targets[i] < targets[0]+2 {
			return 0, 0, reloadRecipeError("flag_aliases_modeword")
		}
		for j := 1; j < i; j++ {
			if targets[i] == targets[j] {
				return 0, 0, reloadRecipeError("normal_flag_alias")
			}
		}
	}
	return targets[3], targets[0], nil
}
