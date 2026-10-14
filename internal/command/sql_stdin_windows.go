package command

import (
	"context"
	"io"
	"os"
	"runtime"
	"time"

	"golang.org/x/sys/windows"
)

var cancelSQLSynchronousIO = windows.NewLazySystemDLL("kernel32.dll").NewProc("CancelSynchronousIo")

// Windows stdin may be a synchronous pipe, file or console. File deadlines and
// Close do not interrupt all of these. Keep the read on a known OS thread so
// cancellation can interrupt synchronous I/O as well as an overlapped handle.
func readSQLStdin(ctx context.Context, input *os.File) ([]byte, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if err := cancelSQLSynchronousIO.Find(); err != nil {
		return nil, err
	}
	conn, err := input.SyscallConn()
	if err != nil {
		return nil, err
	}
	var raw []byte
	var readErr error
	// Hold the File reference until both reading and cancellation have ended.
	// A concurrent Close must not recycle the handle underneath CancelIoEx.
	if err := conn.Control(func(fd uintptr) {
		raw, readErr = readSQLStdinHandle(ctx, input, windows.Handle(fd))
	}); err != nil {
		return nil, err
	}
	return raw, readErr
}

func readSQLStdinHandle(ctx context.Context, input *os.File, handle windows.Handle) ([]byte, error) {
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()
	thread, err := windows.OpenThread(windows.THREAD_TERMINATE, false, windows.GetCurrentThreadId())
	if err != nil {
		return nil, err
	}
	defer windows.CloseHandle(thread)
	finished := make(chan struct{})
	cancelled := make(chan struct{})
	stop := context.AfterFunc(ctx, func() {
		defer close(cancelled)
		ticker := time.NewTicker(time.Millisecond)
		defer ticker.Stop()
		for {
			select {
			case <-finished:
				return
			default:
			}
			// Cancellation can arrive between the context check and ReadFile
			// (ERROR_NOT_FOUND), or between successive reads. Retry until the
			// reader has actually stopped, not just until cancellation is sent.
			cancelSQLSynchronousIO.Call(uintptr(thread))
			windows.CancelIoEx(handle, nil)
			select {
			case <-finished:
				return
			case <-ticker.C:
			}
		}
	})
	defer func() {
		close(finished)
		if !stop() {
			<-cancelled
		}
		// Join before releasing the thread, so cancellation cannot reach an
		// unrelated syscall after the runtime reuses it. Stdin remains open.
	}()
	return io.ReadAll(io.LimitReader(sqlStdinContextReader{ctx, input}, (1<<20)+1))
}

type sqlStdinContextReader struct {
	ctx   context.Context
	input *os.File
}

func (reader sqlStdinContextReader) Read(buf []byte) (int, error) {
	if err := reader.ctx.Err(); err != nil {
		return 0, err
	}
	n, err := reader.input.Read(buf)
	if contextErr := reader.ctx.Err(); contextErr != nil {
		return n, contextErr
	}
	return n, err
}
