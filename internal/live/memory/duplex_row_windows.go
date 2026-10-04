//go:build windows && amd64

package memory

import (
	"context"
	"errors"
	"strings"
	"unsafe"

	"github.com/follenfang/lycheedev/internal/live/duplex"
	"golang.org/x/sys/windows"
)

// PublishDuplexRow is a typed, single-syscall publisher. The protocol message
// selects its row; caller-supplied addresses/raw images are never accepted.
// The guard is an observation gate, not atomicity or allocation lifetime proof.
func (p *Process) PublishDuplexRow(ctx context.Context, r *MailboxReader, m duplex.Message, guard func(context.Context) error) (out duplex.WriteOutcome, facts []DuplexWriteRange, err error) {
	out.State = duplex.NoWrite
	if guard == nil {
		return out, nil, ErrReloadGuardRequired
	}
	if _, bounded := ctx.Deadline(); !bounded {
		return out, nil, errors.New("memory.write_deadline_required")
	}
	if r == nil {
		return out, nil, mailboxError("reader_configuration")
	}
	source, ok := r.source.(*Process)
	if !ok || source.PID != p.PID || source.Created != p.Created || !strings.EqualFold(source.Image, p.Image) {
		return out, nil, errors.New("memory.duplex_writer_identity")
	}
	if err = p.Verify(ctx); err != nil {
		return out, nil, err
	}
	if err = guard(ctx); err != nil {
		return out, nil, err
	}
	if _, err = duplex.EncodeMessage(m); err != nil {
		return out, nil, err
	}
	path, count := duplexMessageRow(m)
	row, err := r.resolveDuplexRow(ctx, path, m.Header.Runtime, m.Header.Arena, count)
	if err != nil {
		return out, nil, err
	}
	plan, err := planDuplexRow(m, row)
	if err != nil {
		return out, nil, err
	}
	verify := func(c context.Context) error {
		a := &luaAccess{reader: row.a.reader, guards: row.a.guards}
		return a.verify(c)
	}
	before := func(c context.Context) error {
		if e := verify(c); e != nil {
			return e
		}
		if e := p.Verify(c); e != nil {
			return e
		}
		// Check every mapping spanned by the full interval. A page check still
		// cannot prove that a Lua object survives a concurrent teardown.
		end := row.address + uint64(len(plan.image))
		if row.address == 0 || end < row.address {
			return errors.New("memory.write_interval_ineligible")
		}
		for at := row.address; at < end; {
			if e := c.Err(); e != nil {
				return e
			}
			var info windows.MemoryBasicInformation
			if e := windows.VirtualQueryEx(p.handle, uintptr(at), &info, unsafe.Sizeof(info)); e != nil {
				return e
			}
			next := uint64(info.BaseAddress + info.RegionSize)
			if next <= at || info.Type != 0x20000 || info.State != windows.MEM_COMMIT || info.Protect != windows.PAGE_READWRITE {
				return errors.New("memory.write_interval_ineligible")
			}
			at = next
		}
		return guard(c)
	}
	write := func(image []byte) (int, error) {
		var n uintptr
		e := windows.WriteProcessMemory(p.handle, uintptr(row.address), &image[0], uintptr(len(image)), &n)
		return int(n), e
	}
	read := func(c context.Context, b []byte) (int, error) { return p.Read(c, row.address, b) }
	var fact DuplexWriteRange
	out, fact, err = executeDuplexRow(ctx, plan, before, write, read, verify)
	return out, []DuplexWriteRange{fact}, err
}
