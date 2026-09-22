//go:build windows

package desktop

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"image"
	"os"
	"os/exec"
	"testing"
	"time"

	"golang.org/x/sys/windows"
)

func TestInputLockChild(t *testing.T) {
	encoded := os.Getenv("LYCHEEDEV_INPUT_LOCK_TARGET")
	if encoded == "" {
		return
	}
	var target WindowIdentity
	if err := json.Unmarshal([]byte(encoded), &target); err != nil {
		t.Fatal(err)
	}
	_, err := withInputLock(context.Background(), target, func() (InputReceipt, error) {
		fmt.Println("held")
		var data [1]byte
		_, err := os.Stdin.Read(data[:])
		return InputReceipt{}, err
	})
	if err != nil {
		t.Fatal(err)
	}
}

func TestInputLockAcrossProcesses(t *testing.T) {
	for _, abandon := range []bool{false, true} {
		t.Run(fmt.Sprint(abandon), func(t *testing.T) {
			f := openFixtureWindow(t, image.NewNRGBA(image.Rect(0, 0, 120, 80)))
			raw, _ := json.Marshal(f.identity)
			child := exec.Command(os.Args[0], "-test.run=^TestInputLockChild$")
			child.Env = append(os.Environ(), "LYCHEEDEV_INPUT_LOCK_TARGET="+string(raw))
			output, err := child.StdoutPipe()
			if err != nil {
				t.Fatal(err)
			}
			input, err := child.StdinPipe()
			if err != nil {
				t.Fatal(err)
			}
			if err := child.Start(); err != nil {
				t.Fatal(err)
			}
			t.Cleanup(func() {
				input.Close()
				if child.ProcessState == nil {
					child.Process.Kill()
					child.Wait()
				}
			})
			ready := make(chan string, 1)
			go func() { line, _ := bufio.NewReader(output).ReadString('\n'); ready <- line }()
			select {
			case line := <-ready:
				if line != "held\n" {
					t.Fatalf("child: %q", line)
				}
			case <-time.After(5 * time.Second):
				t.Fatal("child did not acquire mutex")
			}
			receipt, err := QueueCommand(context.Background(), f.identity, "/dev status")
			if err == nil || err.Error() != "desktop.input_busy" || receipt.MessagesQueued != 0 {
				t.Fatalf("busy sent input: %+v %v", receipt, err)
			}
			prepared := false
			_, err = QueuePreparedCommand(context.Background(), f.identity, func(context.Context) (string, error) { prepared = true; return "/dev status", nil }, func(context.Context) error { return nil })
			if err == nil || err.Error() != "desktop.input_busy" || prepared {
				t.Fatal("busy prepared durable intent", err)
			}
			select {
			case packet := <-f.packets:
				t.Fatalf("busy generated packet: %+v", packet)
			default:
			}
			// A second window does not share this input lock.
			other := openFixtureWindow(t, image.NewNRGBA(image.Rect(0, 0, 120, 80)))
			if _, err := withInputLock(context.Background(), other.identity, func() (InputReceipt, error) { return InputReceipt{}, nil }); err != nil {
				t.Fatal(err)
			}
			if abandon {
				// Keep a handle open so the abandoned kernel object survives its owner.
				name, _ := windows.UTF16PtrFromString(inputMutexName(f.identity))
				handle, err := windows.CreateMutex(nil, false, name)
				if err != nil && !errors.Is(err, windows.ERROR_ALREADY_EXISTS) {
					t.Fatal(err)
				}
				defer windows.CloseHandle(handle)
				if err := child.Process.Kill(); err != nil {
					t.Fatal(err)
				}
				child.Wait()
				receipt, err = QueueCommand(context.Background(), f.identity, "/dev status")
				if err == nil || err.Error() != "desktop.input_abandoned" || receipt.MessagesQueued != 0 {
					t.Fatalf("abandoned sent input: %+v %v", receipt, err)
				}
				return
			}
			if _, err := input.Write([]byte("x")); err != nil {
				t.Fatal(err)
			}
			if err := child.Wait(); err != nil {
				t.Fatal(err)
			}
			receipt, err = QueueCommand(context.Background(), f.identity, "/dev status")
			if err != nil || !receipt.SubmissionComplete {
				t.Fatalf("released: %+v %v", receipt, err)
			}
		})
	}
}
