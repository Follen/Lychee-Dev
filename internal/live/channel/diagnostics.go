package channel

import "errors"

// ErrTraceFlush reports failure to persist diagnostic evidence after the
// authoritative operation has finished. It does not undo a verified result.
var ErrTraceFlush = errors.New("live.channel_trace_flush")
