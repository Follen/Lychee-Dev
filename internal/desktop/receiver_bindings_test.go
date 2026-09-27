package desktop

import "testing"

func TestReceiverBindingsFiniteSetAndFocusedClose(t *testing.T) {
	tests := []struct {
		name     string
		bindings ReceiverBindings
		valid    bool
	}{
		{"defaults", DefaultReceiverBindings(), true},
		{"function keys", ReceiverBindings{"ALT-CTRL-F1", "ALT-CTRL-SHIFT-F2", "ALT-CTRL-F3"}, true},
		{"text key", ReceiverBindings{"ALT-CTRL-A", "ALT-CTRL-SHIFT-]", "ALT-CTRL-["}, false},
		{"noncanonical", ReceiverBindings{"CTRL-ALT-F1", "ALT-CTRL-SHIFT-]", "ALT-CTRL-["}, false},
		{"bare escape", ReceiverBindings{"ESCAPE", "ALT-CTRL-SHIFT-]", "ALT-CTRL-["}, false},
		{"duplicate", ReceiverBindings{"ALT-CTRL-]", "ALT-CTRL-]", "ALT-CTRL-["}, false},
		{"indistinct close", ReceiverBindings{"ALT-CTRL-]", "ALT-CTRL-SHIFT-]", "ALT-CTRL-SHIFT-]"}, false},
		{"distinct chord but shared close key", ReceiverBindings{"ALT-CTRL-[", "ALT-CTRL-SHIFT-]", "ALT-CTRL-SHIFT-["}, false},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			if got := ValidateReceiverBindings(tc.bindings) == nil; got != tc.valid {
				t.Fatalf("valid=%v, want %v", got, tc.valid)
			}
		})
	}
}
