package channel

import (
	"fmt"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
)

const inputSampleGrace = 2 * time.Second

// One sampling period plus the freshness window. This is a discovery hint's
// maximum age, never an input authorization window.
const inputRecentHintMillis int64 = 1500

// This is a local scheduling allowance, never a lease or input authorization.
// Old (even successively newer-but-expired) records cannot renew it. A public
// resume remains bounded by the connection/operation's original durable budget.
type inputSampleWait struct {
	key               string
	started, deadline time.Time
}

func inputSampleKey(e bridge.SlotEnvelope, after int64) string {
	return fmt.Sprintf("%s/%s/%d/%d/%s/%s/%s/%d", e.Runtime, e.Owner, e.Fence, e.Index, e.GUID, e.Build, e.Action, after)
}

func (w *inputSampleWait) waiting(key string, now time.Time) bool {
	return w.key == key && !now.Before(w.started) && now.Before(w.deadline)
}

func (w *inputSampleWait) stale(key string, now time.Time) bool {
	if w.key != key {
		*w = inputSampleWait{key: key, started: now, deadline: now.Add(inputSampleGrace)}
	}
	return w.waiting(key, now)
}

func (w *inputSampleWait) reset() { *w = inputSampleWait{} }
