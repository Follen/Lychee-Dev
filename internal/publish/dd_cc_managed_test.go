package publish

import (
	"context"
	"encoding/base64"
	"encoding/binary"
	"encoding/json"
	"os"
	"testing"
)

type ddManagedFixture struct {
	Base    uint64            `json:"base"`
	Labels  map[string]uint64 `json:"labels"`
	Regions []struct {
		Base  uint64 `json:"base"`
		Bytes string `json:"bytes"`
	} `json:"regions"`
	blocks map[uint64][]byte
	reads  [][2]uint64
}

func ddFixture(t *testing.T) *ddManagedFixture {
	t.Helper()
	raw, err := os.ReadFile("dd_session_testdata/managed.json")
	if err != nil {
		t.Fatal(err)
	}
	f := &ddManagedFixture{}
	if json.Unmarshal(raw, f) != nil {
		t.Fatal("fixture")
	}
	f.blocks = map[uint64][]byte{}
	for _, r := range f.Regions {
		f.blocks[r.Base], err = base64.StdEncoding.DecodeString(r.Bytes)
		if err != nil {
			t.Fatal(err)
		}
	}
	return f
}
func (f *ddManagedFixture) read(address uint64, size int) ([]byte, error) {
	f.reads = append(f.reads, [2]uint64{address, uint64(size)})
	for base, block := range f.blocks {
		if address >= base && address+uint64(size) <= base+uint64(len(block)) {
			return append([]byte(nil), block[address-base:address-base+uint64(size)]...), nil
		}
	}
	return nil, ddManagedError()
}
func (f *ddManagedFixture) write(address uint64, value []byte) {
	for base, block := range f.blocks {
		if address >= base && address+uint64(len(value)) <= base+uint64(len(block)) {
			copy(block[address-base:], value)
			return
		}
	}
	panic("fixture write outside region")
}
func (f *ddManagedFixture) ptr(address, value uint64) {
	var raw [8]byte
	binary.LittleEndian.PutUint64(raw[:], value)
	f.write(address, raw[:])
}
func TestDDManagedIndependentGraphKeepsExactIDsAndUnicode(t *testing.T) {
	f := ddFixture(t)
	reader := &ddManagedReader{ctx: context.Background(), read: f.read}
	s, err := reader.snapshot(f.Base)
	if err != nil {
		t.Fatal(err)
	}
	if s.uid != "18446744073709551615" || s.eid != "100000000000000000000000000001" || s.ccid != "1234" || s.delta != -3.5 || s.sessionValue != "会话😀 é" || s.loginToken != "synthetic-private-login-token" {
		t.Fatal("managed identity differs")
	}
	for key, want := range map[string]string{"latin": "café", "utf16": "中文", "wide": "😀", "noncompact": "非紧凑"} {
		if got := reader.text(f.Labels[key]); got != want {
			t.Fatalf("%s %q", key, got)
		}
	}
	if reader.err != nil {
		t.Fatal(reader.err)
	}
}
func TestDDManagedSessionReplacementAndLoggedOutStateInvalidateOriginal(t *testing.T) {
	for _, change := range []string{"helper_cached_slot", "session_data", "uid", "state_name_slot", "helper_login_slot"} {
		t.Run(change, func(t *testing.T) {
			f := ddFixture(t)
			reader := &ddManagedReader{ctx: context.Background(), read: f.read}
			s, err := reader.snapshot(f.Base)
			if err != nil {
				t.Fatal(err)
			}
			switch change {
			case "helper_cached_slot", "helper_login_slot":
				f.ptr(f.Labels[change], f.Labels["null_token"])
			case "session_data":
				f.write(f.Labels[change], []byte{0x8c, 0x4e, 0, 0})
			case "uid":
				f.write(f.Labels[change]+24, []byte{0, 0, 0, 0})
			case "state_name_slot":
				f.ptr(f.Labels[change], f.Labels["latin"])
			}
			if reader.verify(s) == nil {
				t.Fatal("accepted replaced session")
			}
		})
	}
}
func TestDDManagedMalformedRootsDoNotReadCredentialBytes(t *testing.T) {
	for _, change := range []string{"runtime", "gc", "cycle", "duplicate"} {
		t.Run(change, func(t *testing.T) {
			f := ddFixture(t)
			switch change {
			case "runtime":
				f.write(f.Base+ddPythonRuntimeRVA, []byte{0, 0, 0, 0})
			case "gc":
				f.ptr(f.Base+ddPythonRuntimeRVA+432, f.Base+ddPythonRuntimeRVA+368)
			case "cycle":
				f.ptr(f.Labels["controller_node"], f.Labels["controller_node"])
			case "duplicate":
				raw, _ := f.read(f.Labels["official_module_slot"], 8)
				f.write(f.Labels["other_module_slot"], raw)
			}
			reader := &ddManagedReader{ctx: context.Background(), read: f.read}
			if _, err := reader.snapshot(f.Base); err == nil {
				t.Fatal("accepted malformed graph")
			}
			at := f.Labels["cached_data"]
			for _, read := range f.reads {
				if read[0] <= at && at < read[0]+read[1] {
					t.Fatal("read credential without unique roots")
				}
			}
		})
	}
}
func TestDDGUIJWTBoundToOriginalIdentityAndExpiry(t *testing.T) {
	f := ddFixture(t)
	r := &ddManagedReader{ctx: context.Background(), read: f.read}
	s, err := r.snapshot(f.Base)
	if err != nil {
		t.Fatal(err)
	}
	valid, err := ddGUIJWTValid(s.cached, s, 2000000000)
	if err != nil || !valid {
		t.Fatalf("%v", err)
	}
	s.uid = "other"
	if _, err = ddGUIJWTValid(s.cached, s, 2000000000); err == nil {
		t.Fatal("accepted other account JWT")
	}
	s.uid = "18446744073709551615"
	valid, err = ddGUIJWTValid(s.cached, s, 2000000600)
	if err != nil || valid {
		t.Fatal("expired token accepted")
	}
}
