package live

import "testing"

func TestBootstrapRestageNeverCrossesCommitFence(t *testing.T) {
	for _, phase := range []string{"prepared", "wake_requested", "stage_requested", "submit_requested", "rejected", "commit_requested", "accepted", "confirmed", "abandoned", "dismissed"} {
		for _, ordinal := range []int{0, 1, 2, 3, 4} {
			a := BootstrapReceiverAttempt{Phase: phase, Receiver: receiverAttempt{Phase: phase, Attempt: ordinal}}
			want := (ordinal == 1 || ordinal == 2) && (phase == "wake_requested" || phase == "stage_requested" || phase == "submit_requested" || phase == "rejected")
			if got := bootstrapCanRestage(a); got != want {
				t.Fatalf("phase=%s attempt=%d: got %v", phase, ordinal, got)
			}
			a.Receiver.Phase = "commit_requested"
			if bootstrapCanRestage(a) {
				t.Fatal("contradictory commit evidence permitted resend")
			}
		}
	}
}
