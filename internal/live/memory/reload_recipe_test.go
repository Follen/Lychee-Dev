package memory

import (
	"bytes"
	"context"
	"debug/pe"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"strings"
	"testing"
)

type reloadFixture struct {
	*rootRecipeFixture
	request, worker, world, normal, mode, enabled, blocked, aux uint64
}

func newReloadFixture(t *testing.T, textSize uint64) *reloadFixture {
	f := newRootRecipeFixture(t, 0x1000, textSize, 0x500000)
	r := &reloadFixture{rootRecipeFixture: f, request: f.textRVA + 128, worker: f.textRVA + 256, world: f.textRVA + 384, normal: f.dataRVA + 16, mode: f.dataRVA + 32, enabled: f.dataRVA + 48, blocked: f.dataRVA + 64, aux: f.dataRVA + 80}
	r.put(t, 0, r.request)
	r.put(t, 1, r.worker)
	r.put(t, 2, r.world)
	r.flags(0, (1<<9)|(1<<4))
	return r
}
func (r *reloadFixture) put(t *testing.T, which int, at uint64) {
	t.Helper()
	recipe := []string{retailReloadRequestPattern, retailReloadWorkerPattern, retailReloadWorldPattern}[which]
	code, _ := parseLuaRootPattern(recipe)
	var ops []struct {
		disp, next int
		target     uint64
	}
	switch which {
	case 0:
		ops = []struct {
			disp, next int
			target     uint64
		}{{12, 16, r.mode}, {19, 24, r.enabled}, {28, 33, r.blocked}, {37, 42, r.normal}}
	case 1:
		ops = []struct {
			disp, next int
			target     uint64
		}{{23, 27, r.mode}, {34, 38, r.mode}}
	case 2:
		ops = []struct {
			disp, next int
			target     uint64
		}{{3, 7, r.mode}, {15, 19, r.aux}}
	}
	for _, op := range ops {
		binary.LittleEndian.PutUint32(code[op.disp:], uint32(int32(int64(op.target)-int64(at)-int64(op.next))))
	}
	if at < r.textRVA || at+uint64(len(code)) > r.textRVA+uint64(len(r.text)) {
		t.Fatal("test anchor range")
	}
	copy(r.text[at-r.textRVA:], code)
}
func (r *reloadFixture) flags(request uint8, mode uint16) {
	r.data[r.normal-r.dataRVA] = request
	binary.LittleEndian.PutUint16(r.data[r.mode-r.dataRVA:], mode)
}
func (r *reloadFixture) resolve(t *testing.T, hash, build string) (*ReloadBinding, error) {
	t.Helper()
	layout, e := readLuaRootImageLayout(bytes.NewReader(r.diskHeader), hash, r.size)
	if e != nil {
		t.Fatal(e)
	}
	return ResolveReloadState(context.Background(), r.base, r.size, hash, build, "retail", layout, r.read)
}
func newKnownReloadFixture(t *testing.T) *reloadFixture {
	f := newRootRecipeFixture(t, 0x17bb000, 0x2706d00-0x17bb000, 0x5773000)
	f.data = make([]byte, 0x6067000-f.dataRVA)
	f.size = f.dataRVA + uint64(len(f.data))
	binary.LittleEndian.PutUint32(f.header[0x98+56:], uint32(f.size))
	sections := 0x98 + binary.Size(pe.OptionalHeader64{})
	binary.LittleEndian.PutUint32(f.header[sections+40+8:], uint32(len(f.data)))
	binary.LittleEndian.PutUint32(f.header[sections+40+16:], uint32(len(f.data)))
	f.diskHeader = append([]byte(nil), f.header...)
	r := &reloadFixture{rootRecipeFixture: f, request: retailReloadRequestAnchor, worker: retailReloadWorkerAnchor, world: retailReloadWorldAnchor, normal: retailReloadNormalRVA, mode: retailReloadModeRVA, enabled: 0x59f8fe7, blocked: 0x59f9048, aux: 0x5773ca8}
	for i, raw := range []string{retailReloadRequestHex, retailReloadWorkerHex, retailReloadWorldHex} {
		b, e := hex.DecodeString(raw)
		if e != nil {
			t.Fatal(e)
		}
		at := []uint64{r.request, r.worker, r.world}[i]
		copy(r.text[at-r.textRVA:], b)
	}
	r.flags(0, 529)
	return r
}
func TestReloadKnownAnchorsRequireCodeAndImageProof(t *testing.T) {
	r := newKnownReloadFixture(t)
	b, e := r.resolve(t, retailReloadExecutableSHA256, retailReloadSourceBuild)
	if e != nil {
		t.Fatal(e)
	}
	if b.Resolution.Method != "known_anchors" || b.Resolution.NormalRVA != retailReloadNormalRVA || b.Resolution.ModeRVA != retailReloadModeRVA || b.Resolution.Evidence.ScannedBytes != 0 || b.Resolution.WorldAnchor != retailReloadWorldAnchor {
		t.Fatal("known source targets were not derived", b.Resolution)
	}
	o, e := b.Observe(context.Background())
	if e != nil || o.State != "no_reload_observed" || o.WorldReady == nil || !*o.WorldReady || o.CheckBusinessWriteGate() != nil {
		t.Fatal("known observation failed", o, e)
	}
	r.text[r.request-r.textRVA] ^= 1
	if _, e = r.resolve(t, retailReloadExecutableSHA256, retailReloadSourceBuild); !errors.Is(e, ErrReloadUnknown) {
		t.Fatal("known code mismatch reused fixed RVA", e)
	}
}
func TestReloadCrossBuildRelocatesCompleteUniquePatterns(t *testing.T) {
	for _, cross := range []bool{false, true} {
		r := newReloadFixture(t, 2*luaRootReadChunk+4096)
		if cross {
			clear(r.text)
			r.request = r.textRVA + luaRootReadChunk - 21
			r.worker = r.textRVA + 2*luaRootReadChunk - 15
			r.world = r.textRVA + 2*luaRootReadChunk + 256
			r.put(t, 0, r.request)
			r.put(t, 1, r.worker)
			r.put(t, 2, r.world)
		}
		r.base = 0x180000000
		b, e := r.resolve(t, strings.Repeat("b", 64), "12.2.0.71234")
		if e != nil {
			t.Fatal(e)
		}
		s := b.Resolution
		if s.NormalRVA != r.normal || s.ModeRVA != r.mode || s.ModeRequestAnchor != r.request || s.ModeWorkerAnchor != r.worker || s.NormalAnchor != r.request+35 || s.WorldAnchor != r.world || s.Method != "unique_text_patterns" || s.Evidence.ScannedBytes != uint64(len(r.text)) || s.ExecutableSHA256 == retailReloadExecutableSHA256 || s.Evidence.Validation != "not_run" {
			t.Fatal("new build inherited old address or validation", s)
		}
		o, e := b.Observe(context.Background())
		if e != nil || o.CheckBusinessWriteGate() != nil {
			t.Fatal(e, o)
		}
		if DuplexWriteCapability(s.ExecutableSHA256, s.Build, s.Product).Eligible {
			t.Fatal("read recipe granted write capability")
		}
	}
}
func TestReloadCandidatesFailClosed(t *testing.T) {
	cases := map[string]func(*reloadFixture){
		"missing":           func(r *reloadFixture) { clear(r.text[r.worker-r.textRVA : r.worker-r.textRVA+38]) },
		"duplicate_request": func(r *reloadFixture) { r.put(t, 0, r.textRVA+512) },
		"duplicate_worker":  func(r *reloadFixture) { r.put(t, 1, r.textRVA+512) },
		"two_modewords": func(r *reloadFixture) {
			binary.LittleEndian.PutUint32(r.text[r.worker-r.textRVA+23:], uint32(int32(int64(r.mode+8)-int64(r.worker)-27)))
		},
		"read_store_mismatch": func(r *reloadFixture) {
			binary.LittleEndian.PutUint32(r.text[r.worker-r.textRVA+34:], uint32(int32(int64(r.mode+8)-int64(r.worker)-38)))
		},
		"unaligned_mode": func(r *reloadFixture) { r.mode++; r.put(t, 0, r.request); r.put(t, 1, r.worker) },
		"mode_straddles_data": func(r *reloadFixture) {
			r.mode = r.dataRVA + uint64(len(r.data)) - 1
			r.put(t, 0, r.request)
			r.put(t, 1, r.worker)
		},
		"normal_in_text": func(r *reloadFixture) { r.normal = r.textRVA + 16; r.put(t, 0, r.request) },
		"normal_alias":   func(r *reloadFixture) { r.normal = r.enabled; r.put(t, 0, r.request) },
		"readonly_data": func(r *reloadFixture) {
			at := 0x98 + binary.Size(pe.OptionalHeader64{}) + 40 + 36
			binary.LittleEndian.PutUint32(r.header[at:], pe.IMAGE_SCN_MEM_READ)
			r.diskHeader = append([]byte(nil), r.header...)
		},
		"executable_data": func(r *reloadFixture) {
			at := 0x98 + binary.Size(pe.OptionalHeader64{}) + 40 + 36
			binary.LittleEndian.PutUint32(r.header[at:], pe.IMAGE_SCN_MEM_READ|pe.IMAGE_SCN_MEM_WRITE|pe.IMAGE_SCN_MEM_EXECUTE)
			r.diskHeader = append([]byte(nil), r.header...)
		},
		"text_short_read": func(r *reloadFixture) {
			r.hook = func(at uint64, b []byte) (int, error, bool) { return len(b) - 1, nil, at == r.textRVA }
		},
		"header_short_read": func(r *reloadFixture) {
			r.hook = func(at uint64, b []byte) (int, error, bool) { return len(b) - 1, nil, at < 0x1000 }
		},
		"text_unreadable": func(r *reloadFixture) {
			r.hook = func(at uint64, b []byte) (int, error, bool) { return 0, io.EOF, at == r.textRVA }
		},
		"metadata_changed": func(r *reloadFixture) { r.header[0x98+binary.Size(pe.OptionalHeader64{})+8] ^= 1 },
	}
	for name, mutate := range cases {
		t.Run(name, func(t *testing.T) {
			r := newReloadFixture(t, 4096)
			mutate(r)
			if b, e := r.resolve(t, strings.Repeat("b", 64), "12.2.0.71234"); !errors.Is(e, ErrReloadUnknown) || b != nil {
				t.Fatal("unsafe recipe resolved", b, e)
			}
		})
	}
}
func TestReloadObservationStatesAndPrivateAdmission(t *testing.T) {
	r := newReloadFixture(t, 4096)
	b, e := r.resolve(t, strings.Repeat("b", 64), "12.2.0.71234")
	if e != nil {
		t.Fatal(e)
	}
	for _, tc := range []struct {
		request             uint8
		mode                uint16
		state               string
		gate, errorBusiness error
		world               bool
	}{
		{0, 529, "no_reload_observed", nil, nil, true},
		{1, 529, "requested", ErrReloadActive, ErrReloadActive, true},
		{0, 529 | (1 << 7), "requested", ErrReloadActive, ErrReloadActive, true},
		{0, 529 | (1 << 8), "reloading", ErrReloadActive, ErrReloadActive, true},
		{0, 529 | (1 << 1), "teardown_candidate", ErrReloadUnknown, ErrReloadUnknown, true},
		{0, 1 << 4, "mode_inactive_candidate", ErrReloadUnknown, ErrReloadUnknown, true},
		{0, 1 << 9, "no_reload_observed", nil, ErrNotInWorld, false},
		{0, 529 | (1 << 3), "no_reload_observed", nil, nil, true},
	} {
		r.flags(tc.request, tc.mode)
		o, e := b.Observe(context.Background())
		if e != nil || o.State != tc.state || o.WorldReady == nil || *o.WorldReady != tc.world || !errors.Is(o.CheckWriteGate(), tc.gate) || !errors.Is(o.CheckBusinessWriteGate(), tc.errorBusiness) {
			t.Fatalf("flags %#x request %d: %+v %v", tc.mode, tc.request, o, e)
		}
	}
	r.flags(0, 529)
	o, e := b.Observe(context.Background())
	if e != nil {
		t.Fatal(e)
	}
	wire, _ := json.Marshal(o)
	var decoded ReloadObservation
	if e = json.Unmarshal(wire, &decoded); e != nil {
		t.Fatal(e)
	}
	if decoded.CheckWriteGate() == nil {
		t.Fatal("JSON manufactured private admission")
	}
	*o.WorldReady = false
	if o.CheckWriteGate() == nil {
		t.Fatal("edited world projection admitted")
	}
	o, e = b.Observe(context.Background())
	if e != nil {
		t.Fatal(e)
	}
	*o.Mode = 0
	if o.CheckWriteGate() == nil {
		t.Fatal("edited flags admitted")
	}
	b.Resolution.ModeRVA++
	if _, e = b.Observe(context.Background()); !errors.Is(e, ErrReloadUnknown) {
		t.Fatal("edited binding admitted", e)
	}
}
func TestReloadWorldQualificationIndependentOfCleanup(t *testing.T) {
	cases := map[string]func(*reloadFixture){
		"missing":   func(r *reloadFixture) { clear(r.text[r.world-r.textRVA : r.world-r.textRVA+23]) },
		"duplicate": func(r *reloadFixture) { r.put(t, 2, r.textRVA+640) },
		"different_mode": func(r *reloadFixture) {
			binary.LittleEndian.PutUint32(r.text[r.world-r.textRVA+3:], uint32(int32(int64(r.mode+8)-int64(r.world)-7)))
		},
		"bad_aux_width": func(r *reloadFixture) { r.aux = r.dataRVA + uint64(len(r.data)) - 4; r.put(t, 2, r.world) },
		"changed_shift": func(r *reloadFixture) { r.text[r.world-r.textRVA+9] = 9 },
	}
	for name, mutate := range cases {
		t.Run(name, func(t *testing.T) {
			r := newReloadFixture(t, 4096)
			mutate(r)
			b, e := r.resolve(t, strings.Repeat("b", 64), "12.2.0.71234")
			if e != nil {
				t.Fatal("world qualification erased reload proof", e)
			}
			o, e := b.Observe(context.Background())
			if e != nil || o.State != "no_reload_observed" || o.WorldReady != nil || o.WorldState != "unknown" || o.CheckWriteGate() != nil || !errors.Is(o.CheckBusinessWriteGate(), ErrWorldUnknown) {
				t.Fatal("unknown world incorrectly admitted business or blocked cleanup", o, e)
			}
		})
	}
}
func TestReloadChangedReadGuardsRejectUnknown(t *testing.T) {
	for _, which := range []string{"flags", "anchor", "metadata", "world"} {
		t.Run(which, func(t *testing.T) {
			r := newReloadFixture(t, 4096)
			b, e := r.resolve(t, strings.Repeat("b", 64), "12.2.0.71234")
			if e != nil {
				t.Fatal(e)
			}
			switch which {
			case "flags":
				reads := 0
				r.hook = func(at uint64, p []byte) (int, error, bool) {
					if at != r.normal {
						return 0, nil, false
					}
					reads++
					p[0] = byte(reads - 1)
					return len(p), nil, true
				}
			case "anchor":
				r.text[r.request-r.textRVA] ^= 1
			case "metadata":
				r.header[0x98+binary.Size(pe.OptionalHeader64{})+8] ^= 1
			case "world":
				r.text[r.world-r.textRVA+9] ^= 1
			}
			o, e := b.Observe(context.Background())
			if which == "world" {
				if e != nil || o.WorldReady != nil || o.CheckWriteGate() != nil || !errors.Is(o.CheckBusinessWriteGate(), ErrWorldUnknown) {
					t.Fatal("world drift blocked cleanup or admitted business", o, e)
				}
			} else if !errors.Is(e, ErrReloadUnknown) || !errors.Is(o.CheckWriteGate(), ErrReloadUnknown) {
				t.Fatal("changed sample admitted", o, e)
			}
		})
	}
}
func TestReloadRequiresMatchingLockedLayoutAndHonorsContext(t *testing.T) {
	r := newReloadFixture(t, 4096)
	hash := strings.Repeat("b", 64)
	layout, e := readLuaRootImageLayout(bytes.NewReader(r.diskHeader), hash, r.size)
	if e != nil {
		t.Fatal(e)
	}
	if _, e = ResolveReloadState(context.Background(), r.base, r.size, hash, "12.2.0.71234", "retail", LuaRootImageLayout{}, r.read); !errors.Is(e, ErrReloadUnknown) {
		t.Fatal("missing locked layout admitted")
	}
	if _, e = ResolveReloadState(context.Background(), r.base, r.size, strings.Repeat("c", 64), "12.2.0.71234", "retail", layout, r.read); !errors.Is(e, ErrReloadUnknown) {
		t.Fatal("old hash proof admitted")
	}
	if _, e = ResolveReloadState(context.Background(), r.base, r.size, hash, "12.2.0.71234", "classic", layout, r.read); !errors.Is(e, ErrReloadUnknown) {
		t.Fatal("product guessed")
	}
	ctx, cancel := context.WithCancel(context.Background())
	r.hook = func(at uint64, p []byte) (int, error, bool) {
		if at == r.textRVA {
			cancel()
			copy(p, r.text)
			return len(p), nil, true
		}
		return 0, nil, false
	}
	if _, e = ResolveReloadState(ctx, r.base, r.size, hash, "12.2.0.71234", "retail", layout, r.read); !errors.Is(e, context.Canceled) {
		t.Fatal("scan ignored cancellation", e)
	}
}
