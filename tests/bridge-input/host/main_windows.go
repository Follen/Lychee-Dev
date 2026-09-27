//go:build windows && amd64 && lycheedev_input_lab

package main

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"hash/adler32"
	"image"
	"image/draw"
	"image/png"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"time"

	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/live"
	"github.com/follenfang/lycheedev/internal/live/journal"
)

type signal struct {
	Token, Build, Boot   string
	Sequence             int
	State, Nonce, Digest string
	Bytes, Commits       int
	Focus                bool
}

func parseSignal(raw string) (signal, bool) {
	p := strings.Split(raw, "|")
	if len(p) != 11 || p[0] != "LDIL1" {
		return signal{}, false
	}
	sequence, e1 := strconv.Atoi(p[4])
	size, e2 := strconv.Atoi(p[8])
	commits, e3 := strconv.Atoi(p[9])
	if e1 != nil || e2 != nil || e3 != nil || sequence < 1 || size < 0 || size > 128 || commits < 0 || (p[10] != "0" && p[10] != "1") {
		return signal{}, false
	}
	return signal{p[1], p[2], p[3], sequence, p[5], p[6], p[7], size, commits, p[10] == "1"}, true
}

type runner struct {
	ctx          context.Context
	target       live.ClientWindow
	stream       *desktop.FrameStream
	token, out   string
	events       []any
	after        time.Time
	cleanupOwner string
}

// The independent lab card is below the production top-left receipt. Scan
// bounded overlapping left-hand bands so the small-module fallback can find it.
func decodeLabSymbols(frame image.Image) ([]string, error) {
	texts, err := desktop.DecodeSymbols(frame)
	for _, text := range texts {
		if strings.HasPrefix(text, "LDIL1|") {
			return texts, nil
		}
	}
	b := frame.Bounds()
	for y := 128; y < b.Dy() && y < 1024; y += 128 {
		crop := image.NewNRGBA(image.Rect(0, 0, min(512, b.Dx()), min(512, b.Dy()-y)))
		draw.Draw(crop, crop.Bounds(), frame, image.Pt(b.Min.X, b.Min.Y+y), draw.Src)
		found, _ := desktop.DecodeSymbols(crop)
		for _, text := range found {
			if strings.HasPrefix(text, "LDIL1|") {
				return found, nil
			}
		}
	}
	return texts, err
}

func (r *runner) save(name string, v any) error {
	b, err := json.MarshalIndent(v, "", "  ")
	if err != nil {
		return err
	}
	return os.WriteFile(filepath.Join(r.out, name+".json"), append(b, '\n'), 0600)
}
func (r *runner) capture(name string, f image.Image) error {
	p, err := os.Create(filepath.Join(r.out, name+".png"))
	if err != nil {
		return err
	}
	return errors.Join(png.Encode(p, f), p.Close())
}
func (r *runner) wait(stage string, seconds int, accept func(signal) bool) (signal, error) {
	ctx, cancel := context.WithTimeout(r.ctx, time.Duration(seconds)*time.Second)
	defer cancel()
	var last *desktop.CapturedFrame
	var lastDecode time.Time
	for {
		f, err := r.stream.Next(ctx)
		if err != nil {
			if last != nil {
				_ = r.capture(stage+"-last", last.NRGBA)
			}
			return signal{}, fmt.Errorf("lab.%s: %w", stage, err)
		}
		last = f
		if !r.after.IsZero() && !f.ObservedAt.After(r.after) {
			continue
		}
		if time.Since(lastDecode) < 150*time.Millisecond {
			continue
		}
		lastDecode = time.Now()
		if err := desktop.ConfirmWindow(ctx, r.target.Window); err != nil {
			return signal{}, err
		}
		texts, err := decodeLabSymbols(f.NRGBA)
		if err != nil {
			continue
		}
		for _, raw := range texts {
			s, ok := parseSignal(string(desktop.BytesFromSymbolText(raw)))
			if !ok || s.Token != r.token {
				continue
			}
			if !accept(s) && (s.State == "binding_conflict" || s.State == "combat") {
				return s, fmt.Errorf("lab.receiver_%s", s.State)
			}
			if accept(s) {
				r.events = append(r.events, map[string]any{"stage": stage, "observedAt": f.ObservedAt, "signal": s, "targetForeground": desktop.LabForeground(r.target.Window)})
				if err := r.capture(stage, f.NRGBA); err != nil {
					return signal{}, err
				}
				if err := r.save("observations", r.events); err != nil {
					return signal{}, err
				}
				return s, nil
			}
		}
	}
}
func (r *runner) guard(ctx context.Context) error {
	resource := fmt.Sprintf("window/%d/%d/%d", r.target.Window.ProcessID, r.target.Window.ProcessStartedAt, r.target.Window.Handle)
	owner, occupied, err := journal.InspectWindowOwner(ctx, filepath.Join(r.target.Client.Directory, "Interface", "AddOns"), resource)
	if err != nil {
		return err
	}
	if occupied {
		// The user-authorized cooperating owner can allow only removal of our
		// own observed lab UI. No probe/reload can use this narrow exception.
		if owner.OperationID == r.cleanupOwner {
			return desktop.ConfirmWindow(ctx, r.target.Window)
		}
		return fmt.Errorf("lab.window_owned: %s", owner.OperationID)
	}
	return desktop.ConfirmWindow(ctx, r.target.Window)
}
func (r *runner) clear(in *desktop.LabInput) error {
	if err := in.Chord("dismiss"); err != nil {
		return err
	}
	return r.verifyClear()
}

func (r *runner) verifyClear() error {
	after := time.Now()
	ctx, cancel := context.WithTimeout(r.ctx, 5*time.Second)
	defer cancel()
	consecutive := 0
	for {
		f, err := r.stream.Next(ctx)
		if err != nil {
			return err
		}
		if !f.ObservedAt.After(after) {
			continue
		}
		texts, err := decodeLabSymbols(f.NRGBA)
		if err != nil {
			return err
		}
		found := false
		for _, raw := range texts {
			if s, ok := parseSignal(string(desktop.BytesFromSymbolText(raw))); ok && s.Token == r.token {
				found = true
			}
		}
		if found {
			consecutive = 0
			continue
		}
		consecutive++
		if consecutive == 2 {
			r.events = append(r.events, map[string]any{"stage": "cleared", "observedAt": f.ObservedAt})
			return r.capture("cleared", f.NRGBA)
		}
	}
}
func (r *runner) probe(in *desktop.LabInput) (err error) {
	baseline, baselineErr := r.wait("baseline", 2, func(s signal) bool {
		return s.State == "idle" || s.State == "accepted" || s.State == "timeout" || s.State == "focus_lost" || s.State == "rejected"
	})
	if baselineErr != nil && !errors.Is(baselineErr, context.DeadlineExceeded) {
		return baselineErr
	}
	if err = in.Chord("wake"); err != nil {
		return err
	}
	r.after = time.Now()
	ready, err := r.wait("wake", 5, func(s signal) bool {
		return (baselineErr != nil || (s.Boot == baseline.Boot && s.Sequence > baseline.Sequence)) && s.State == "ready" && s.Focus
	})
	if err != nil {
		return err
	} // Never send text without an observed focused receiver.
	defer func() {
		cleanup := r.clear(in)
		err = errors.Join(err, cleanup)
		if cleanup != nil {
			r.events = append(r.events, map[string]any{"stage": "cleanup_pending", "error": cleanup.Error()})
		}
	}()
	b := make([]byte, 8)
	if _, err = rand.Read(b); err != nil {
		return err
	}
	body := "echo_" + hex.EncodeToString(b)
	wire := "LDIL1:" + ready.Nonce + ":" + body
	wire = fmt.Sprintf("%s:%08x", wire, adler32.Checksum([]byte(wire)))
	if err = r.save("expected", map[string]any{"body": body, "wire": wire, "nonce": ready.Nonce}); err != nil {
		return err
	}
	if err = in.Text(wire); err != nil {
		return err
	}
	r.after = time.Now()
	digest := fmt.Sprintf("%08x", adler32.Checksum([]byte(body)))
	staged, err := r.wait("staged", 5, func(s signal) bool {
		return s.Boot == ready.Boot && s.Sequence > ready.Sequence && s.State == "staged" && s.Nonce == ready.Nonce && s.Digest == digest && s.Bytes == len(body) && s.Focus
	})
	if err != nil {
		return err
	}
	if err = in.Chord("submit"); err != nil {
		return err
	}
	r.after = time.Now()
	_, err = r.wait("accepted", 5, func(s signal) bool {
		return s.Boot == staged.Boot && s.Sequence > staged.Sequence && s.State == "accepted" && s.Nonce == ready.Nonce && s.Digest == digest && s.Bytes == len(body) && s.Commits == ready.Commits+1 && !s.Focus
	})
	if err != nil {
		return err
	}
	// Repeated submit may not accept again. A later capture must still name the
	// same accepted transaction and counter; there is no Lua evaluation in this lab.
	if err = in.Chord("submit"); err != nil {
		return err
	}
	r.after = time.Now()
	if err = in.Wait(300 * time.Millisecond); err != nil {
		return err
	}
	_, err = r.wait("duplicate-submit", 3, func(s signal) bool {
		return s.Boot == staged.Boot && s.State == "accepted" && s.Nonce == ready.Nonce && s.Digest == digest && s.Bytes == len(body) && s.Commits == ready.Commits+1 && !s.Focus
	})
	return err
}
func run() int {
	pid := flag.Uint("pid", 0, "explicit WoW process ID")
	installation := flag.String("installation", "", "explicit client directory")
	action := flag.String("action", "inspect", "inspect, observe, probe, reload, dismiss, or hide-lab-chat")
	mode := flag.String("mode", "postmessage", "postmessage or sendinput (target must already be foreground)")
	out := flag.String("out", "", "new evidence directory; must not already exist")
	cleanupOwner := flag.String("cleanup-owner", "", "coordinated owner ID; dismiss our observed lab receipt only")
	captureArea := flag.String("capture-area", "receipt", "inspect capture area: receipt or window")
	flag.Parse()
	if *captureArea != "receipt" && *captureArea != "window" {
		fmt.Fprintln(os.Stderr, "--capture-area must be receipt or window")
		return 2
	}
	if *captureArea == "window" && *action != "inspect" {
		fmt.Fprintln(os.Stderr, "--capture-area window is only valid with inspect")
		return 2
	}
	if *cleanupOwner != "" && *action != "dismiss" && *action != "hide-lab-chat" {
		fmt.Fprintln(os.Stderr, "--cleanup-owner is only valid for fixture cleanup")
		return 2
	}
	if *pid == 0 || *pid > 1<<32-1 || *installation == "" || *out == "" {
		fmt.Fprintln(os.Stderr, "--pid, --installation and a new --out are required")
		return 2
	}
	switch *action {
	case "inspect", "observe", "probe", "reload", "dismiss", "hide-lab-chat":
	default:
		fmt.Fprintln(os.Stderr, "invalid action")
		return 2
	}
	if err := os.Mkdir(*out, 0700); err != nil {
		fmt.Fprintln(os.Stderr, err)
		return 2
	}
	ctx, cancel := context.WithTimeout(context.Background(), 55*time.Second)
	defer cancel()
	r := &runner{ctx: ctx, out: *out, cleanupOwner: *cleanupOwner}
	result := map[string]any{"schema": "lycheedev.input-lab.v1", "action": *action, "mode": *mode, "passed": false, "startedAt": time.Now().UTC()}
	var receipt desktop.InputReceipt
	err := func() error {
		var err error
		r.target, err = live.ResolveClientWindow(ctx, *installation, uint32(*pid))
		if err != nil {
			return err
		}
		result["target"] = r.target
		result["keyboardLayout"] = desktop.LabKeyboardLayout(r.target.Window)
		result["targetForegroundAtStart"] = desktop.LabForeground(r.target.Window)
		if err = r.guard(ctx); err != nil && *action != "inspect" && *action != "observe" {
			return err
		}
		if *action != "inspect" {
			bytes, err := os.ReadFile(filepath.Join(*installation, "Interface", "AddOns", "LycheeInputLab", "lab-manifest.json"))
			if err != nil {
				return err
			}
			var m struct {
				Token string `json:"token"`
			}
			if err = json.Unmarshal(bytes, &m); err != nil {
				return err
			}
			r.token = m.Token
			if len(r.token) != 16 {
				return errors.New("lab.invalid_install_token")
			}
		}
		area := image.Rectangle{}
		if *captureArea == "window" {
			area = desktop.WholeWindowCapture()
		}
		result["captureArea"] = *captureArea
		r.stream, err = desktop.CaptureFrames(ctx, r.target.Window, area)
		if err != nil {
			return err
		}
		defer r.stream.Close()
		if *action == "inspect" {
			f, err := r.stream.Next(ctx)
			if err != nil {
				return err
			}
			return r.capture("before", f.NRGBA)
		}
		if *action == "observe" {
			_, err = r.wait("observe", 8, func(s signal) bool { return true })
			return err
		}
		if (*action == "dismiss" && *cleanupOwner != "") || *action == "hide-lab-chat" {
			if _, err = r.wait("owned-lab-receipt", 4, func(s signal) bool { return true }); err != nil {
				return err
			}
			result["coordinatedCleanupOwner"] = *cleanupOwner
		}
		if err = r.save("intent", result); err != nil {
			return err
		}
		receipt, err = desktop.WithLabInput(ctx, r.target.Window, *mode, r.guard, func(in *desktop.LabInput) error {
			switch *action {
			case "probe":
				return r.probe(in)
			case "reload":
				// A visible idle receipt from before reload is not new activation.
				previousBoot := ""
				before, captureErr := r.stream.Next(ctx)
				if captureErr != nil {
					return captureErr
				}
				texts, decodeErr := decodeLabSymbols(before.NRGBA)
				if decodeErr != nil {
					return decodeErr
				}
				for _, raw := range texts {
					if s, ok := parseSignal(string(desktop.BytesFromSymbolText(raw))); ok && s.Token == r.token {
						previousBoot = s.Boot
					}
				}
				if err := in.Reload(); err != nil {
					return err
				}
				activated, err := r.wait("activated", 30, func(s signal) bool {
					return s.Boot != previousBoot && (s.State == "idle" || s.State == "binding_conflict" || s.State == "combat")
				})
				if err == nil && activated.State != "idle" {
					return fmt.Errorf("lab.activation_%s", activated.State)
				}
				return err
			case "dismiss":
				return r.clear(in)
			case "hide-lab-chat":
				if err := in.HideLabViaChat(); err != nil {
					return err
				}
				return r.verifyClear()
			}
			return errors.New("lab.invalid_action")
		})
		return err
	}()
	result["input"] = receipt
	result["observations"] = r.events
	result["finishedAt"] = time.Now().UTC()
	result["passed"] = err == nil
	if err != nil {
		result["error"] = err.Error()
	}
	if e := r.save("result", result); e != nil {
		fmt.Fprintln(os.Stderr, e)
		return 5
	}
	b, _ := json.MarshalIndent(result, "", "  ")
	fmt.Println(string(b))
	if err != nil {
		return 6
	}
	return 0
}
func main() { os.Exit(run()) }
