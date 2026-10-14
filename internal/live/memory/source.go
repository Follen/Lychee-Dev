// Package memory reads fixed runtime roots and named Lua Mailbox publications.
package memory

import "context"

type Range struct {
	Start uint64 `json:"start,string"`
	End   uint64 `json:"end,string"`
}

type Source interface {
	Verify(context.Context) error
	Read(context.Context, uint64, []byte) (int, error)
}

type Gap struct {
	Range
	Reason string `json:"reason"`
}

type Worker struct {
	PlannedBytes   uint64 `json:"plannedBytes"`
	ScannedBytes   uint64 `json:"scannedBytes"`
	Complete       bool   `json:"complete"`
	Truncated      bool   `json:"truncated"`
	SkippedRegions int    `json:"skippedRegions"`
	Gaps           []Gap  `json:"gaps"`
	ReadCalls      uint64 `json:"readCalls"`
	Candidates     uint64 `json:"candidates"`
}

type Coverage struct {
	StoppedEarly   bool     `json:"stoppedEarly"`
	Reenumerated   bool     `json:"reenumerated"`
	PlannedBytes   uint64   `json:"plannedBytes"`
	ScannedBytes   uint64   `json:"scannedBytes"`
	Complete       bool     `json:"complete"`
	Truncated      bool     `json:"truncated"`
	SkippedRegions int      `json:"skippedRegions"`
	Gaps           []Gap    `json:"gaps"`
	Workers        []Worker `json:"workers"`
	ElapsedMillis  int64    `json:"elapsedMillis"`
}

type LookupResult struct {
	FirstReadFailure *ReadFailure `json:"firstReadFailure,omitempty"`
	Records          []Record     `json:"records"`
	Coverage         Coverage     `json:"coverage"`
	Path             string       `json:"path"`
	HintMillis       int64        `json:"hintMillis"`
	ElapsedMillis    int64        `json:"elapsedMillis"`
	Fallback         string       `json:"fallback,omitempty"`
}

// ReadFailure contains bounded diagnostics from an already attempted read.
// Paths are fixed logical labels, never addresses, requested keys or contents.
// Missing region/IO fields mean the adapter supplied no such evidence.
type ReadFailure struct {
	Path            string      `json:"path"`
	RequestedBytes  int         `json:"requestedBytes"`
	ReturnedBytes   int         `json:"returnedBytes"`
	Source          string      `json:"source"`
	Reason          string      `json:"reason"`
	Region          *ReadRegion `json:"region,omitempty"`
	IOAttempted     *bool       `json:"ioAttempted,omitempty"`
	NativeErrorCode uint32      `json:"nativeErrorCode,omitempty"`
	GuardRecheck    string      `json:"guardRecheck"`
}

type ReadRegion struct {
	Type    uint32 `json:"type"`
	State   uint32 `json:"state"`
	Protect uint32 `json:"protect"`
}

// Preserve the original error text and chain; metadata grants no read authority.
type readFailureError struct {
	cause   error
	details ReadFailure
}

func (e *readFailureError) Error() string { return e.cause.Error() }
func (e *readFailureError) Unwrap() error { return e.cause }
