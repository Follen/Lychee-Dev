//go:build windows

package luals

import (
	"encoding/binary"
	"golang.org/x/sys/windows"
	"path/filepath"
	"strings"
	"sync"
	"unicode/utf16"
)

// A directory watcher has one overlapped read. Stop cancels that read and joins
// the dispatch goroutine before releasing its handles. Retirement callbacks run
// after dispatch exits, so callback Close cannot wait on itself.
func directoryWatch(dir string, changed func([]byte) bool, retire func()) (func(), error) {
	path, err := windows.UTF16PtrFromString(dir)
	if err != nil {
		return nil, err
	}
	handle, err := windows.CreateFile(path, windows.FILE_LIST_DIRECTORY, windows.FILE_SHARE_READ|windows.FILE_SHARE_WRITE|windows.FILE_SHARE_DELETE, nil, windows.OPEN_EXISTING, windows.FILE_FLAG_BACKUP_SEMANTICS|windows.FILE_FLAG_OVERLAPPED, 0)
	if err != nil {
		return nil, err
	}
	event, err := windows.CreateEvent(nil, 1, 0, nil)
	if err != nil {
		windows.CloseHandle(handle)
		return nil, err
	}
	done := make(chan struct{})
	ready := make(chan struct{})
	var readyOnce sync.Once
	stopping := make(chan struct{})
	var once sync.Once
	var submit sync.Mutex
	stop := func() {
		once.Do(func() {
			close(stopping)
			submit.Lock()
			_ = windows.CancelIoEx(handle, nil)
			submit.Unlock()
			<-done
			windows.CloseHandle(handle)
			windows.CloseHandle(event)
		})
	}
	go func() {
		failure := false
		defer func() {
			readyOnce.Do(func() { close(ready) })
			close(done)
			if failure {
				go retire()
			}
		}()
		buffer := make([]byte, 64<<10)
		for events := 0; events < 4096; events++ {
			overlapped := windows.Overlapped{HEvent: event}
			var n uint32
			submit.Lock()
			select {
			case <-stopping:
				submit.Unlock()
				return
			default:
			}
			_ = windows.ResetEvent(event)
			err := windows.ReadDirectoryChanges(handle, &buffer[0], uint32(len(buffer)), true, windows.FILE_NOTIFY_CHANGE_FILE_NAME|windows.FILE_NOTIFY_CHANGE_DIR_NAME|windows.FILE_NOTIFY_CHANGE_SIZE|windows.FILE_NOTIFY_CHANGE_LAST_WRITE, &n, &overlapped, 0)
			submit.Unlock()
			readyOnce.Do(func() { close(ready) })
			if err != nil && err != windows.ERROR_IO_PENDING {
				failure = true
				return
			}
			if _, err = windows.WaitForSingleObject(event, windows.INFINITE); err != nil {
				_ = windows.CancelIoEx(handle, &overlapped)
				_ = windows.GetOverlappedResult(handle, &overlapped, &n, true)
				failure = true
				return
			}
			err = windows.GetOverlappedResult(handle, &overlapped, &n, false)
			select {
			case <-stopping:
				return
			default:
			}
			if err != nil || n == 0 || changed(buffer[:n]) {
				failure = true
				return
			}
		}
		failure = true
	}()
	<-ready
	return stop, nil
}
func watchSessionArtifacts(dir string, retire func()) (func(), error) {
	return directoryWatch(dir, func(events []byte) bool { return sealedArtifactEvent(events) || checkSessionArtifacts(dir) != nil }, retire)
}
func watchWorkspace(dir string, retire func()) (func(), error) {
	return directoryWatch(dir, func([]byte) bool { return true }, retire)
}
func WatchIdentityDirectory(dir string, retire func()) (func(), error) {
	return watchWorkspace(dir, retire)
}

// Writes and renames of the generated library/config invalidate even when the
// previous bytes are restored before the next admission. Log/meta events only
// need the separate capacity check.
func sealedArtifactEvent(events []byte) bool {
	for offset := 0; offset < len(events); {
		if len(events)-offset < 12 {
			return true
		}
		next := int(binary.LittleEndian.Uint32(events[offset:]))
		length := int(binary.LittleEndian.Uint32(events[offset+8:]))
		if length%2 != 0 || length > len(events)-offset-12 {
			return true
		}
		name := make([]uint16, length/2)
		for i := range name {
			name[i] = binary.LittleEndian.Uint16(events[offset+12+i*2:])
		}
		path := strings.ToLower(filepath.ToSlash(string(utf16.Decode(name))))
		action := binary.LittleEndian.Uint32(events[offset+4:])
		if path == "config.json" || path == "definitions" && action != windows.FILE_ACTION_MODIFIED || strings.HasPrefix(path, "definitions/") {
			return true
		}
		if next == 0 {
			return false
		}
		if next < 12 || next > len(events)-offset {
			return true
		}
		offset += next
	}
	return false
}
