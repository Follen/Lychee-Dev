package desktop

import (
	"context"
	"errors"
	"strings"
	"testing"
	"time"
	"unicode/utf16"
)

func TestBackgroundTextPacingAndInterruptedSubmission(t *testing.T) {
	units, err := commandUnits("/dev 中文 😀")
	if err != nil {
		t.Fatal(err)
	}
	for stop := -1; stop < 2*len(units)+3; stop++ {
		step, enters, typed := 0, 0, 0
		interrupted := errors.New("interrupted")
		check := func() error {
			current := step
			step++
			if current == stop {
				return interrupted
			}
			return nil
		}
		err := transmitText(units, func() error {
			if err := check(); err != nil {
				return err
			}
			enters++
			return nil
		}, func(unit uint16) error {
			if err := check(); err != nil {
				return err
			}
			if enters != 1 || unit != units[typed] {
				t.Fatal("changed input order or UTF-16 units")
			}
			typed++
			return nil
		}, func(delay time.Duration) error {
			if err := check(); err != nil {
				return err
			}
			want := 50 * time.Millisecond
			if typed == 0 {
				want = 150 * time.Millisecond
			}
			if delay != want {
				t.Fatalf("input pacing = %v, want %v", delay, want)
			}
			return nil
		})
		if stop < 0 {
			if err != nil || enters != 2 || typed != len(units) {
				t.Fatalf("incomplete input: %v %d %d", err, enters, typed)
			}
		} else if !errors.Is(err, interrupted) || enters > 1 || step != stop+1 {
			t.Fatalf("continued or submitted interrupted input: stop=%d step=%d enters=%d err=%v", stop, step, enters, err)
		}
	}
}

func TestCommandUnitsPreserveUTF16(t *testing.T) {
	command := "/dev 中文 😀"
	units, err := commandUnits(command)
	if err != nil {
		t.Fatal(err)
	}
	if string(utf16.Decode(units)) != command {
		t.Fatalf("%v", units)
	}
	if len(units) != len([]rune(command))+1 {
		t.Fatal("non-BMP scalar was not a surrogate pair")
	}
	for _, invalid := range []string{"hello", "/one\n/two", "/bad\ttext", "/" + strings.Repeat("中", 85), "/\xff"} {
		if _, err := commandUnits(invalid); err == nil {
			t.Fatalf("accepted %q", invalid)
		}
	}
}

func TestBootstrapEntryAcceptsOnlyFixedStrings(t *testing.T) {
	nonce := "0123456789abcdef0123456789abcdef"
	for _, allowed := range []string{"/dev connect", "/dev bridge identify " + nonce, "/dev bridge reset " + nonce} {
		if err := bootstrapCommand(allowed); err != nil {
			t.Fatalf("rejected %q: %v", allowed, err)
		}
	}
	for _, rejected := range []string{
		"",
		"dev connect",
		"/dev connect ",
		" /dev connect",
		"/dev Connect",
		"/dev connect extra",
		"/dev disconnect",
		"/dev bridge ready",
		"/dev bridge hide",
		"/dev bridge reset",
		"/dev bridge reset ",
		"/dev bridge reset " + nonce[:31],
		"/dev bridge reset " + nonce + "0",
		"/dev bridge reset " + strings.ToUpper(nonce),
		"/dev bridge reset " + strings.Repeat("z", 32),
		"/dev bridge reset " + nonce + " extra",
		"/dev bridge bind " + nonce,
		"/dev bridge load Request-A",
		"/dev bridge identify",
		"/dev bridge identify ",
		"/dev bridge identify " + nonce[:31],
		"/dev bridge identify " + nonce + "0",
		"/dev bridge identify " + strings.ToUpper(nonce),
		"/dev bridge identify " + strings.Repeat("z", 32),
		"/dev bridge identify 0123456789abcdef0123456789abcdeF",
		"/dev bridge identify " + nonce + " ",
		"/dev bridge identify " + nonce + "\n/dev connect",
		"/dev bridge identifyX" + nonce,
		"/run print(1)",
	} {
		if err := bootstrapCommand(rejected); err == nil {
			t.Fatalf("accepted %q", rejected)
		}
	}
}

func TestBootstrapCommandRejectsTextBeforeAnyPlatformInput(t *testing.T) {
	ctx := context.Background()
	target := WindowIdentity{Handle: 1, ProcessID: 2, ProcessStartedAt: 3}
	// The allowlist runs before the platform entry: invalid text is rejected
	// with the same hard error on every platform, never queued as input.
	for _, rejected := range []string{"0123456789abcdef0123456789abcdef", "/dev connect\tx", "/dev connect\n"} {
		err, ok := error(nil), false
		_, err = QueueBootstrapCommand(ctx, target, rejected)
		ok = err != nil && err.Error() == "desktop.bootstrap_command_not_allowed"
		if !ok {
			t.Fatalf("bootstrap text %q: %v", rejected, err)
		}
	}
}
