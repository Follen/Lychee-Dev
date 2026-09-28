//go:build windows && amd64 && lycheedev_input_lab

package main

import (
	"context"
	"encoding/json"
	"flag"
	"fmt"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/live"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"image/png"
	"os"
	"path/filepath"
	"regexp"
	"time"
)

func main() {
	if err := run(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
func run() error {
	pid := flag.Uint("pid", 0, "explicit WoW PID")
	installation := flag.String("installation", "", "explicit client")
	command := flag.String("command", "", "whitelisted lab command; blank is capture only")
	out := flag.String("out", "", "new evidence prefix")
	flag.Parse()
	allowed := map[string]bool{"": true, "/memlab2 status": true, "/memlab2 next": true, "/memlab2 reset": true, "/memlab2 clear": true, "/memlab clear": true, "/memlod replay": true, "/memlod status": true, "/memlod stop": true, "LOAD": true}
	allowed["/noncelab clear"] = true
	if regexp.MustCompile(`^/noncelab publish [0-9a-f]{32}$`).MatchString(*command) {
		allowed[*command] = true
	}
	if !allowed[*command] || *pid == 0 || *installation == "" || *out == "" {
		return fmt.Errorf("invalid lab selection")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 25*time.Second)
	defer cancel()
	target, err := live.ResolveClientWindow(ctx, *installation, uint32(*pid))
	if err != nil {
		return err
	}
	guard := func(ctx context.Context) error {
		resource := fmt.Sprintf("window/%d/%d/%d", target.Window.ProcessID, target.Window.ProcessStartedAt, target.Window.Handle)
		owner, busy, err := journal.InspectWindowOwner(ctx, filepath.Join(target.Client.Directory, "Interface", "AddOns"), resource)
		if err != nil {
			return err
		}
		if busy {
			return fmt.Errorf("owned by %s", owner.OperationID)
		}
		return desktop.ConfirmWindow(ctx, target.Window)
	}
	if err = guard(ctx); err != nil {
		return err
	}
	if err = os.MkdirAll(filepath.Dir(*out), 0700); err != nil {
		return err
	}
	if _, err = os.Stat(*out + ".json"); err == nil {
		return fmt.Errorf("evidence already exists")
	}
	stream, err := desktop.CaptureFrames(ctx, target.Window, desktop.WholeWindowCapture())
	if err != nil {
		return err
	}
	defer stream.Close()
	save := func(suffix string) error {
		frame, err := stream.Next(ctx)
		if err != nil {
			return err
		}
		file, err := os.OpenFile(*out+suffix+".png", os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
		if err != nil {
			return err
		}
		defer file.Close()
		return png.Encode(file, frame.NRGBA)
	}
	if err = save("-before"); err != nil {
		return err
	}
	var receipt desktop.InputReceipt
	if *command != "" {
		receipt, err = desktop.WithLabInput(ctx, target.Window, "postmessage", guard, func(in *desktop.LabInput) error {
			if *command == "LOAD" {
				return in.ReceiverChord("ALT-CTRL-F9")
			}
			for i := 0; i < 3; i++ {
				if e := in.Chord("escape"); e != nil {
					return e
				}
			}
			if e := in.Chord("enter"); e != nil {
				return e
			}
			if e := in.Text(*command); e != nil {
				return e
			}
			return in.Chord("enter")
		})
	}
	after := time.Now().Add(500 * time.Millisecond)
	for time.Now().Before(after) {
		if _, e := stream.Next(ctx); e != nil {
			return e
		}
	}
	capErr := save("-after")
	data, _ := json.MarshalIndent(map[string]any{"target": target, "command": *command, "receipt": receipt, "error": fmt.Sprint(err), "at": time.Now()}, "", "  ")
	if e := os.WriteFile(*out+".json", data, 0600); e != nil {
		return e
	}
	fmt.Println(string(data))
	if err != nil {
		return err
	}
	return capErr
}
