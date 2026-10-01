package luals

import (
	"os/exec"
	"sync"
	"syscall"
	"unsafe"

	"golang.org/x/sys/windows"
)

func hideProcess(command *exec.Cmd) { command.SysProcAttr = &syscall.SysProcAttr{HideWindow: true} }

// A job with kill-on-close also owns workers spawned by public --check.
// Attaching is mandatory: failing closed avoids leaving an unowned child.
func guardProcess(command *exec.Cmd) (func(), error) { return guardProcessPolicy(command, false) }
func guardProcessPolicy(command *exec.Cmd, broker bool) (func(), error) {
	job, err := windows.CreateJobObject(nil, nil)
	if err != nil {
		return nil, err
	}
	limit := windows.JOBOBJECT_EXTENDED_LIMIT_INFORMATION{}
	limit.BasicLimitInformation.LimitFlags = windows.JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
	_, err = windows.SetInformationJobObject(job, windows.JobObjectExtendedLimitInformation,
		uintptr(unsafe.Pointer(&limit)), uint32(unsafe.Sizeof(limit)))
	if err != nil {
		windows.CloseHandle(job)
		return nil, err
	}
	if broker {
		if err = setBrokerJobMemory(job); err != nil {
			windows.CloseHandle(job)
			return nil, err
		}
	}
	process, err := windows.OpenProcess(windows.PROCESS_SET_QUOTA|windows.PROCESS_TERMINATE, false, uint32(command.Process.Pid))
	if err != nil {
		windows.CloseHandle(job)
		return nil, err
	}
	err = windows.AssignProcessToJobObject(job, process)
	windows.CloseHandle(process)
	if err != nil {
		windows.CloseHandle(job)
		return nil, err
	}
	if command.SysProcAttr.CreationFlags&windows.CREATE_SUSPENDED != 0 {
		snapshot, err := windows.CreateToolhelp32Snapshot(windows.TH32CS_SNAPTHREAD, 0)
		if err != nil {
			windows.CloseHandle(job)
			return nil, err
		}
		entry := windows.ThreadEntry32{Size: uint32(unsafe.Sizeof(windows.ThreadEntry32{}))}
		found := false
		for err = windows.Thread32First(snapshot, &entry); err == nil; err = windows.Thread32Next(snapshot, &entry) {
			if entry.OwnerProcessID == uint32(command.Process.Pid) {
				thread, e := windows.OpenThread(windows.THREAD_SUSPEND_RESUME, false, entry.ThreadID)
				if e != nil {
					windows.CloseHandle(snapshot)
					windows.CloseHandle(job)
					return nil, e
				}
				_, e = windows.ResumeThread(thread)
				windows.CloseHandle(thread)
				if e != nil {
					windows.CloseHandle(snapshot)
					windows.CloseHandle(job)
					return nil, e
				}
				found = true
				break
			}
		}
		windows.CloseHandle(snapshot)
		if !found {
			windows.CloseHandle(job)
			return nil, windows.ERROR_NOT_FOUND
		}
	}
	var once sync.Once
	return func() { once.Do(func() { windows.CloseHandle(job) }) }, nil
}

// Suspend before assigning the Job: the server cannot spawn an escaping worker
// between CreateProcess and mandatory custody.
func prepareGuarded(command *exec.Cmd) {
	if command.SysProcAttr == nil {
		command.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}
	}
	command.SysProcAttr.CreationFlags |= windows.CREATE_SUSPENDED
}
