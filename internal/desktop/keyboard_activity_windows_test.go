//go:build windows && amd64

package desktop

import "testing"

func TestKeyboardActivityInstallsAndClosesNativeHook(t *testing.T) {
	for range 2 {
		m, err := NewKeyboardActivity()
		if err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() { _ = m.Close() })
		if _, ok := keyboardListeners.Load(m.threadID); !ok {
			t.Fatal("native hook was not registered")
		}
		if err := m.Close(); err != nil {
			t.Fatal(err)
		}
		if _, ok := keyboardListeners.Load(m.threadID); ok {
			t.Fatal("closed hook retained its listener")
		}
		if err := m.Check(); err == nil || err.Error() != "desktop.keyboard_monitor_closed" {
			t.Fatalf("closed hook authorized input: %v", err)
		}
	}
}
