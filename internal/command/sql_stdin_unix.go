//go:build linux || darwin

package command

import (
	"context"
	"errors"
	"os"
	"time"

	"golang.org/x/sys/unix"
)

// Inherited stdin is not necessarily registered with Go's poller, so neither
// SetReadDeadline nor closing it reliably interrupts Read. Read nonblocking on
// an owned descriptor instead; there is no background reader to leave behind.
func readSQLStdin(ctx context.Context, input *os.File) (raw []byte, err error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	conn, err := input.SyscallConn()
	if err != nil {
		return nil, err
	}
	fd := -1
	var duplicateErr error
	if err := conn.Control(func(source uintptr) {
		fd, duplicateErr = unix.FcntlInt(source, unix.F_DUPFD_CLOEXEC, 0)
	}); err != nil {
		return nil, err
	}
	if duplicateErr != nil {
		return nil, duplicateErr
	}
	defer unix.Close(fd)
	flags, err := unix.FcntlInt(uintptr(fd), unix.F_GETFL, 0)
	if err != nil {
		return nil, err
	}
	if flags&unix.O_NONBLOCK == 0 {
		if err := unix.SetNonblock(fd, true); err != nil {
			return nil, err
		}
		// Dup shares file status flags with stdin; leave the caller's mode as it
		// was, including for inherited stdin in an embedding process.
		defer func() {
			if restoreErr := unix.SetNonblock(fd, false); err == nil {
				err = restoreErr
			}
		}()
	}
	// Polling only when no bytes are ready avoids platform-specific pipe/TTY
	// poller restrictions. Cancellation wakes this wait immediately.
	ticker := time.NewTicker(10 * time.Millisecond)
	defer ticker.Stop()
	var buf [32 << 10]byte
	for len(raw) < (1<<20)+1 {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		remaining := min(len(buf), (1<<20)+1-len(raw))
		n, readErr := unix.Read(fd, buf[:remaining])
		if n > 0 {
			raw = append(raw, buf[:n]...)
		}
		if readErr == nil && n == 0 {
			return raw, ctx.Err()
		}
		if errors.Is(readErr, unix.EINTR) {
			continue
		}
		if errors.Is(readErr, unix.EAGAIN) || errors.Is(readErr, unix.EWOULDBLOCK) {
			select {
			case <-ctx.Done():
				return nil, ctx.Err()
			case <-ticker.C:
				continue
			}
		}
		if readErr != nil {
			return nil, readErr
		}
	}
	return raw, ctx.Err()
}
