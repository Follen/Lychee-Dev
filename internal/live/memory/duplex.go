package memory

import (
	"context"
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"errors"
	"io"
	"math"

	"github.com/follenfang/lycheedev/internal/live/duplex"
)

const DuplexMailboxSchema = "lycheedev.mailbox.v1"

// Array layout is checked against a runtime calibration, never inherited from
// the root RVA alone. A calibration is evidence of this instance's layout, not
// a collector lifetime pin. No heap address is kept across publications.
type DuplexPath struct {
	Name  string
	Index int
}

type NumericCell struct {
	Address uint64
	Value   uint32
}

type DuplexArray struct {
	Cells []NumericCell
	a     *luaAccess
}

func (v *DuplexArray) Verify(ctx context.Context) error {
	// Each bounded slice rechecks the same ephemeral locating proof. Read
	// budgets apply independently to a slice; the publisher bounds slice count.
	a := &luaAccess{reader: v.a.reader, guards: v.a.guards}
	return a.verify(ctx)
}

func (r *MailboxReader) duplexRoot(ctx context.Context) (*luaAccess, uint64, error) {
	a := &luaAccess{reader: r}
	if err := r.source.Verify(ctx); err != nil {
		return nil, 0, err
	}
	if r.binding.proof != nil {
		at := r.root - r.binding.RootRVA + r.binding.AnchorRVA
		b, err := a.read(ctx, at, r.binding.Evidence.PatternBytes, true)
		if err != nil {
			return nil, 0, err
		}
		h := sha256.Sum256(b)
		if hex.EncodeToString(h[:]) != r.binding.Evidence.AnchorSHA256 {
			return nil, 0, mailboxError("root_recipe_changed")
		}
		a.guard(at, b, true)
	}
	b, err := a.read(ctx, r.root, 8, true)
	if err != nil {
		return nil, 0, err
	}
	a.guard(r.root, b, true)
	state := binary.LittleEndian.Uint64(b)
	b, err = a.read(ctx, state+16, 1, false)
	if err != nil {
		return nil, 0, err
	}
	if state == 0 || b[0] != 8 {
		return nil, 0, mailboxError("lua_state_tag")
	}
	a.guard(state+16, b, false)
	b, err = a.read(ctx, state+0x90, 24, false)
	if err != nil {
		return nil, 0, err
	}
	if b[8] != 5 || b[9] != 0 {
		return nil, 0, mailboxError("globals_layout")
	}
	a.guard(state+0x90, b[:10], false)
	v, err := a.lookup(ctx, binary.LittleEndian.Uint64(b), "LycheeDevInternal")
	if err != nil || v.tag != 5 {
		return nil, 0, errors.Join(mailboxError("namespace_layout"), err)
	}
	v, err = a.lookup(ctx, v.pointer, "Mailbox")
	if err != nil || v.tag != 5 {
		return nil, 0, errors.Join(mailboxError("mailbox_layout"), err)
	}
	box := v.pointer
	for _, check := range []struct{ name, want string }{{"schema", DuplexMailboxSchema}, {"layoutId", duplex.MailboxLayoutID}, {"release", r.release}} {
		value, e := a.lookup(ctx, box, check.name)
		if e != nil || value.tag != 4 {
			return nil, 0, errors.Join(mailboxError("duplex_"+check.name), e)
		}
		text, e := a.string(ctx, value.pointer, 64, true)
		if e != nil || string(text) != check.want {
			return nil, 0, errors.Join(mailboxError("duplex_"+check.name+"_mismatch"), e)
		}
	}
	return a, box, nil
}

func (a *luaAccess) arrayRange(ctx context.Context, table uint64, first, count int) (uint64, []byte, error) {
	if first < 1 || count < 1 || count > 2048 {
		return 0, nil, mailboxError("array_range")
	}
	at, err := a.arrayLocation(ctx, table, first, count)
	if err != nil {
		return 0, nil, err
	}
	b, err := a.read(ctx, at, count*24, false)
	return at, b, err
}

func (a *luaAccess) arrayLocation(ctx context.Context, table uint64, first, count int) (uint64, error) {
	h, err := a.read(ctx, table, 72, false)
	if err != nil {
		return 0, err
	}
	size := uint64(binary.LittleEndian.Uint32(h[64:68]))
	array := binary.LittleEndian.Uint64(h[32:40])
	last := uint64(first-1) + uint64(count)
	if h[16] != 5 || size > 1<<20 || last > size || array == 0 || array > ^uint64(0)-size*24 {
		return 0, mailboxError("array_layout_unsupported")
	}
	a.guard(table+16, h[16:17], false)
	a.guard(table+64, h[64:68], false)
	a.guard(table+32, h[32:40], false)
	return array + uint64(first-1)*24, nil
}

func (a *luaAccess) duplexArrayRange(ctx context.Context, table uint64, count int) (uint64, []byte, error) {
	if count <= 2048 {
		return a.arrayRange(ctx, table, 1, count)
	}
	// Only the exact full command row receives this dedicated TValue budget.
	// General array/hash/string reads keep their 2048/1MiB/512-call bounds.
	if count != duplexCommandWords {
		return 0, nil, mailboxError("duplex_row_budget")
	}
	at, err := a.arrayLocation(ctx, table, 1, count)
	if err != nil {
		return 0, nil, err
	}
	if a.calls >= 512 || ctx.Err() != nil {
		return 0, nil, errors.Join(mailboxError("read_budget"), ctx.Err())
	}
	a.calls++
	b := make([]byte, count*24)
	n, err := a.reader.source.Read(ctx, at, b)
	if err != nil || n != len(b) {
		err = errors.Join(mailboxError("short_read"), err, io.ErrUnexpectedEOF, ctx.Err())
	}
	return at, b, err
}

func (a *luaAccess) duplexPath(ctx context.Context, table uint64, path []DuplexPath) (luaValue, error) {
	if len(path) == 0 || len(path) > 6 {
		return luaValue{}, mailboxError("duplex_path")
	}
	value := luaValue{pointer: table, tag: 5}
	for _, part := range path {
		if value.tag != 5 {
			return luaValue{}, mailboxError("duplex_path_type")
		}
		if part.Name != "" && part.Index == 0 {
			var err error
			value, err = a.lookup(ctx, value.pointer, part.Name)
			if err != nil {
				return luaValue{}, err
			}
		} else if part.Name == "" && part.Index > 0 {
			at, b, err := a.arrayRange(ctx, value.pointer, part.Index, 1)
			if err != nil {
				return luaValue{}, err
			}
			if b[9] != 0 {
				return luaValue{}, mailboxError("restricted_array_value")
			}
			a.guard(at, b[:10], false)
			value = luaValue{pointer: binary.LittleEndian.Uint64(b), tag: b[8]}
		} else {
			return luaValue{}, mailboxError("duplex_path")
		}
	}
	return value, nil
}

// ReadDuplexString resolves a fresh immutable sendbox publication. A changed
// root or field pointer rejects the entire sample; it is not a heap search.
func (r *MailboxReader) ReadDuplexString(ctx context.Context, path []DuplexPath, maximum int) ([]byte, error) {
	if maximum < 1 || maximum > 600<<10 {
		return nil, mailboxError("duplex_string_budget")
	}
	for attempt := 0; attempt < 3; attempt++ {
		b, err := r.readDuplexStringOnce(ctx, path, maximum)
		if !errors.Is(err, ErrMailboxPathChanged) || ctx.Err() != nil {
			return b, err
		}
	}
	return nil, ErrMailboxPathChanged
}

func (r *MailboxReader) readDuplexStringOnce(ctx context.Context, path []DuplexPath, maximum int) ([]byte, error) {
	a, box, err := r.duplexRoot(ctx)
	if err != nil {
		return nil, err
	}
	v, err := a.duplexPath(ctx, box, path)
	if err != nil || v.tag != 4 {
		return nil, errors.Join(mailboxError("duplex_string_unavailable"), err)
	}
	b, err := a.string(ctx, v.pointer, uint64(maximum), false)
	if err != nil {
		return nil, err
	}
	if err = a.verify(ctx); err != nil {
		return nil, err
	}
	return b, nil
}

func (r *MailboxReader) ResolveDuplexArray(ctx context.Context, path []DuplexPath, runtime, arena string, count int) (*DuplexArray, error) {
	row, err := r.resolveDuplexRow(ctx, path, runtime, arena, count)
	if err != nil {
		return nil, err
	}
	result := &DuplexArray{a: row.a, Cells: make([]NumericCell, count)}
	for i := range result.Cells {
		cell := row.image[i*24 : (i+1)*24]
		value := math.Float64frombits(binary.LittleEndian.Uint64(cell))
		if cell[8] != 3 || cell[9] != 0 || value < 0 || value > math.MaxUint32 || math.Trunc(value) != value || math.IsNaN(value) {
			return nil, mailboxError("duplex_numeric_cell")
		}
		result.Cells[i] = NumericCell{Address: row.address + uint64(i*24), Value: uint32(value)}
	}
	return result, nil
}

func (r *MailboxReader) resolveDuplexRow(ctx context.Context, path []DuplexPath, runtime, arena string, count int) (*duplexRow, error) {
	a, box, err := r.duplexRoot(ctx)
	if err != nil {
		return nil, err
	}
	for _, check := range []struct{ name, want string }{{"runtime", runtime}, {"arenaGeneration", arena}} {
		v, e := a.lookup(ctx, box, check.name)
		if e != nil || v.tag != 4 {
			return nil, errors.Join(mailboxError("duplex_identity"), e)
		}
		b, e := a.string(ctx, v.pointer, 32, true)
		if e != nil || string(b) != check.want {
			return nil, errors.Join(mailboxError("duplex_identity_changed"), e)
		}
	}
	cal, err := a.duplexPath(ctx, box, []DuplexPath{{Name: "inbox"}, {Name: "calibration"}})
	if err != nil || cal.tag != 5 {
		return nil, errors.Join(mailboxError("duplex_calibration"), err)
	}
	at, b, err := a.arrayRange(ctx, cal.pointer, 1, 6)
	if err != nil {
		return nil, err
	}
	for i, expected := range []float64{0, 1, 4294967295, 0.125, -13.5, 7654321} {
		cell := b[i*24 : (i+1)*24]
		if cell[8] != 3 || cell[9] != 0 || math.Float64frombits(binary.LittleEndian.Uint64(cell)) != expected {
			return nil, mailboxError("numeric_layout_unsupported")
		}
	}
	a.guard(at, b, false)
	v, err := a.duplexPath(ctx, box, path)
	if err != nil || v.tag != 5 {
		return nil, errors.Join(mailboxError("duplex_array_type"), err)
	}
	at, b, err = a.duplexArrayRange(ctx, v.pointer, count)
	if err != nil {
		return nil, err
	}
	flag, err := a.read(ctx, v.pointer+0x45, 1, false)
	if err != nil {
		return nil, err
	}
	a.guard(v.pointer+0x45, flag, false)
	if err = a.verify(ctx); err != nil {
		return nil, err
	}
	return &duplexRow{a: a, address: at, image: b, frozen: flag[0] == 1}, nil
}

func numericPayload(value uint32) []byte {
	b := make([]byte, 8)
	binary.LittleEndian.PutUint64(b, math.Float64bits(float64(value)))
	return b
}
