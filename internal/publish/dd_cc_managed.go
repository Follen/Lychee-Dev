package publish

import (
	"context"
	"encoding/binary"
	"math"
	"math/big"
	"strings"
	"time"
	"unicode"
	"unicode/utf16"
	"unicode/utf8"
)

const ddPythonRuntimeRVA uint64 = 3639528

type ddManagedReader struct {
	ctx   context.Context
	read  func(uint64, int) ([]byte, error)
	types map[uint64]string
	err   error
}

// These values stay in the optional channel operation; none is serialized.
type ddManagedSession struct {
	controller, helper, center, session, token, dataCenter, user, login, deltaObject uint64
	sessionValue, cached, loginToken, uid, eid, ccid                                 string
	delta                                                                            float64
}

func ddManagedError() error {
	return capability("publish.dd_gui_channel", "the selected DD GUI session changed or differs from the pinned CPython 3.7 layout")
}
func (r *ddManagedReader) fail() {
	if r.err == nil {
		r.err = ddManagedError()
	}
}
func (r *ddManagedReader) bytes(addr uint64, size int) []byte {
	if r.err != nil {
		return make([]byte, max(size, 0))
	}
	if err := r.ctx.Err(); err != nil {
		r.err = err
		return make([]byte, max(size, 0))
	}
	if addr < 0x10000 || addr >= 0x800000000000 || size <= 0 || size > 1<<20 || uint64(size) > 0x800000000000-addr {
		r.fail()
		return make([]byte, min(max(size, 0), 1<<20))
	}
	raw, err := r.read(addr, size)
	if err != nil || len(raw) != size {
		r.fail()
		return make([]byte, size)
	}
	return raw
}
func (r *ddManagedReader) pointer(addr uint64) uint64 {
	v := binary.LittleEndian.Uint64(r.bytes(addr, 8))
	if v != 0 && (v < 0x10000 || v >= 0x800000000000) {
		r.fail()
	}
	return v
}
func (r *ddManagedReader) integer(addr uint64) int64 {
	return int64(binary.LittleEndian.Uint64(r.bytes(addr, 8)))
}
func (r *ddManagedReader) typeName(object uint64) string {
	typ := r.pointer(object + 8)
	if value, ok := r.types[typ]; ok {
		return value
	}
	name := r.pointer(typ + 24)
	raw := r.bytes(name, 128)
	end := 0
	for end < len(raw) && raw[end] != 0 {
		end++
	}
	value := string(raw[:end])
	if r.types == nil {
		r.types = map[uint64]string{}
	}
	r.types[typ] = value
	return value
}
func (r *ddManagedReader) text(object uint64) string {
	if r.typeName(object) != "str" {
		r.fail()
		return ""
	}
	header := r.bytes(object, 48)
	length := int64(binary.LittleEndian.Uint64(header[16:]))
	state := binary.LittleEndian.Uint32(header[32:])
	kind := int((state >> 2) & 7)
	compact, ascii := state&(1<<5) != 0, state&(1<<6) != 0
	if length < 0 || length > 16384 || (kind != 1 && kind != 2 && kind != 4) || (ascii && (kind != 1 || !compact)) {
		r.fail()
		return ""
	}
	if length == 0 {
		return ""
	}
	var data uint64
	if compact {
		data = object + 72
		if ascii {
			data = object + 48
		}
	} else {
		data = r.pointer(object + 72)
	}
	raw := r.bytes(data, int(length)*kind)
	out := make([]rune, 0, int(length))
	switch kind {
	case 1:
		for _, b := range raw {
			out = append(out, rune(b))
		}
	case 2:
		values := make([]uint16, len(raw)/2)
		for i := range values {
			values[i] = binary.LittleEndian.Uint16(raw[i*2:])
		}
		for i := 0; i < len(values); i++ {
			v := values[i]
			if utf16.IsSurrogate(rune(v)) {
				if i+1 >= len(values) || v < 0xd800 || v > 0xdbff || values[i+1] < 0xdc00 || values[i+1] > 0xdfff {
					r.fail()
					return ""
				}
				out = append(out, utf16.DecodeRune(rune(v), rune(values[i+1])))
				i++
			} else {
				out = append(out, rune(v))
			}
		}
	case 4:
		for i := 0; i < len(raw); i += 4 {
			v := rune(binary.LittleEndian.Uint32(raw[i:]))
			if !utf8.ValidRune(v) {
				r.fail()
				return ""
			}
			out = append(out, v)
		}
	}
	return string(out)
}
func (r *ddManagedReader) dict(object uint64, wanted string) uint64 {
	if r.typeName(object) != "dict" {
		r.fail()
		return 0
	}
	keys, values := r.pointer(object+32), r.pointer(object+40)
	size, count := r.integer(keys+8), r.integer(keys+32)
	if size <= 0 || size > 1048576 || size&(size-1) != 0 || count < 0 || count > 16384 {
		r.fail()
		return 0
	}
	width := uint64(4)
	if size <= 255 {
		width = 1
	} else if size <= 65535 {
		width = 2
	}
	entries := keys + 40 + uint64(size)*width
	for i := uint64(0); i < uint64(count) && r.err == nil; i++ {
		key := r.pointer(entries + i*24 + 8)
		if key != 0 && r.text(key) == wanted {
			if values != 0 {
				return r.pointer(values + i*8)
			}
			return r.pointer(entries + i*24 + 16)
		}
	}
	return 0
}
func (r *ddManagedReader) attr(object uint64, name string) uint64 {
	typ := r.pointer(object + 8)
	offset := r.integer(typ + 288)
	if offset < 0 {
		basic, item := r.integer(typ+32), r.integer(typ+40)
		if basic < 0 || item < 0 || basic > 65536 || item > 65536 {
			r.fail()
			return 0
		}
		count := int64(0)
		if item != 0 {
			count = r.integer(object + 16)
			if count < 0 {
				count = -count
			}
			if count < 0 || count > 65536 {
				r.fail()
				return 0
			}
		}
		offset += (basic + count*item + 7) &^ 7
	}
	if offset <= 0 || offset >= 65536 {
		r.fail()
		return 0
	}
	dict := r.pointer(object + uint64(offset))
	if dict == 0 {
		return 0
	}
	return r.dict(dict, name)
}
func (r *ddManagedReader) scalar(object uint64) (string, float64, bool) {
	switch r.typeName(object) {
	case "str":
		return r.text(object), 0, false
	case "float":
		value := math.Float64frombits(binary.LittleEndian.Uint64(r.bytes(object+16, 8)))
		if math.IsNaN(value) || math.IsInf(value, 0) {
			r.fail()
		}
		return "", value, true
	case "int":
		size := r.integer(object + 16)
		if size < -4 || size > 4 {
			r.fail()
			return "", 0, false
		}
		negative := size < 0
		if negative {
			size = -size
		}
		value := new(big.Int)
		for i := int64(0); i < size; i++ {
			digit := binary.LittleEndian.Uint32(r.bytes(object+24+uint64(i)*4, 4))
			if digit >= 1<<30 {
				r.fail()
				return "", 0, false
			}
			part := new(big.Int).Lsh(new(big.Int).SetUint64(uint64(digit)), uint(i*30))
			value.Or(value, part)
		}
		if negative {
			value.Neg(value)
		}
		f, _ := new(big.Float).SetInt(value).Float64()
		return value.String(), f, true
	default:
		r.fail()
		return "", 0, false
	}
}
func (r *ddManagedReader) id(object uint64) string {
	s, _, _ := r.scalar(object)
	if s == "" || len(s) > 30 {
		r.fail()
	}
	for _, c := range s {
		if c < '0' || c > '9' {
			r.fail()
		}
	}
	return s
}
func (r *ddManagedReader) module(object uint64) string {
	typ := r.pointer(object + 8)
	dict := r.pointer(typ + 264)
	name := r.dict(dict, "__module__")
	if name == 0 {
		return ""
	}
	return r.text(name)
}
func (r *ddManagedReader) loggedIn(center uint64) {
	if r.typeName(center) != "MessageCenter" || r.module(center) != "components.net_server.messagecenter" {
		r.fail()
		return
	}
	state := r.attr(center, "state")
	if state == 0 || r.module(state) != "components.net_server.messagecenter" {
		r.fail()
		return
	}
	name := r.text(r.attr(state, "_name_"))
	value, _, _ := r.scalar(r.attr(state, "_value_"))
	if name != "LoggedIn" || value != "3" {
		r.fail()
	}
}
func (r *ddManagedReader) objects(base uint64) (uint64, uint64) {
	runtime := base + ddPythonRuntimeRVA
	if binary.LittleEndian.Uint32(r.bytes(runtime, 4)) != 1 || r.pointer(runtime+432) != runtime+336 {
		r.fail()
		return 0, 0
	}
	controller, helper := uint64(0), uint64(0)
	seen := map[uint64]bool{}
	deadline := time.Now().Add(10 * time.Second)
	for generation := uint64(0); generation < 3; generation++ {
		head := runtime + 336 + generation*32
		node := r.pointer(head)
		for node != head && r.err == nil {
			if node == 0 || len(seen) >= 300000 || seen[node] || time.Now().After(deadline) {
				r.fail()
				break
			}
			seen[node] = true
			next := r.pointer(node)
			object := node + 24
			name := r.typeName(object)
			if name == "LoginController" && r.module(object) == "components.login.login_controller" {
				if controller != 0 {
					r.fail()
				}
				controller = object
			} else if name == "JwtHelper" && r.module(object) == "util.cgi.jwt_helper" {
				if helper != 0 {
					r.fail()
				}
				helper = object
			}
			node = next
		}
	}
	if controller == 0 || helper == 0 {
		r.fail()
	}
	return controller, helper
}
func (r *ddManagedReader) session(controller, helper uint64) ddManagedSession {
	s := ddManagedSession{controller: controller, helper: helper}
	s.center = r.attr(controller, "_messageCenter")
	if s.center == 0 || r.attr(helper, "_msgCenter") != s.center {
		r.fail()
	}
	r.loggedIn(s.center)
	s.session = r.attr(controller, "_session")
	s.sessionValue = r.text(s.session)
	if s.sessionValue == "" {
		r.fail()
	}
	s.token = r.attr(helper, "_jwt")
	if s.token != 0 && r.typeName(s.token) != "NoneType" {
		s.cached = r.text(s.token)
	}
	s.dataCenter = r.attr(helper, "_dataCenter")
	s.user = r.attr(s.dataCenter, "_myUserInfo")
	s.login = r.attr(helper, "_loginToken")
	s.loginToken = r.text(s.login)
	s.deltaObject = r.attr(helper, "_server_time_delta")
	_, s.delta, _ = r.scalar(s.deltaObject)
	if r.typeName(s.deltaObject) != "int" && r.typeName(s.deltaObject) != "float" || s.delta < -86400 || s.delta > 86400 {
		r.fail()
	}
	s.uid = r.id(r.attr(s.user, "uid"))
	s.eid = r.id(r.attr(s.user, "eid"))
	s.ccid = r.id(r.attr(s.user, "ccid"))
	if s.loginToken == "" || strings.IndexFunc(s.loginToken, unicode.IsControl) >= 0 || strings.IndexFunc(s.cached, unicode.IsControl) >= 0 {
		r.fail()
	}
	return s
}
func (r *ddManagedReader) snapshot(base uint64) (ddManagedSession, error) {
	controller, helper := r.objects(base)
	s := r.session(controller, helper)
	if r.err != nil {
		return s, r.err
	}
	if r.session(controller, helper) != s {
		r.fail()
	}
	return s, r.err
}
func (r *ddManagedReader) verify(s ddManagedSession) error {
	if r.session(s.controller, s.helper) != s {
		r.fail()
	}
	return r.err
}
func ddDecimal(s string) bool {
	if s == "" || len(s) > 30 {
		return false
	}
	for _, c := range s {
		if c < '0' || c > '9' {
			return false
		}
	}
	return true
}
