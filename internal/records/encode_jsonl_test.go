package records_test

import (
	"bytes"
	"encoding/json"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/records"
)

func decodeJSONLFrames(t *testing.T, raw string) []records.JSONLFrame {
	t.Helper()
	lines := strings.Split(strings.TrimSuffix(raw, "\n"), "\n")
	frames := make([]records.JSONLFrame, 0, len(lines))
	for _, line := range lines {
		if line == "" {
			continue
		}
		var frame records.JSONLFrame
		if err := json.Unmarshal([]byte(line), &frame); err != nil {
			t.Fatalf("frame %q: %v", line, err)
		}
		frames = append(frames, frame)
	}
	return frames
}

func TestWriteHotfixJSONLFrames(t *testing.T) {
	result := records.RemoteHotfixResult{
		Schema:   records.RemoteHotfixResultSchema,
		Query:    wagoTestQuery(),
		Snapshot: "PIN-1",
		Records: []records.HotfixRecord{
			{Provider: "wago", ID: 11, Push: 100, RecordID: 55, Status: 1, Build: "68974"},
			{Provider: "wago", ID: 12, Push: 100, RecordID: 56, Status: 1, Build: "68974"},
		},
		Coverage: records.HotfixCoverage{Provider: "wago", Complete: true},
		Warnings: []string{"records.wago_cache_hit"},
		Complete: true,
	}
	var buffer bytes.Buffer
	if err := records.WriteHotfixJSONL(&buffer, result); err != nil {
		t.Fatal(err)
	}
	frames := decodeJSONLFrames(t, buffer.String())
	if len(frames) != 4 {
		t.Fatalf("frames = %d", len(frames))
	}
	for _, frame := range frames {
		if frame.Schema != records.JSONLSchema || frame.OperationID != "PIN-1" {
			t.Fatalf("frame envelope = %+v", frame)
		}
	}
	if frames[0].Type != records.JSONLFrameBegin || len(frames[0].Context) == 0 {
		t.Fatalf("begin = %+v", frames[0])
	}
	var context struct {
		Query    records.HotfixQuery `json:"query"`
		Coverage records.HotfixCoverage
	}
	if err := json.Unmarshal(frames[0].Context, &context); err != nil || context.Query.Product != "retail" {
		t.Fatalf("begin context = %v err = %v", context, err)
	}
	for position := 1; position <= 2; position++ {
		if frames[position].Type != records.JSONLFrameRecord || frames[position].Index == nil || *frames[position].Index != position-1 || len(frames[position].Record) == 0 {
			t.Fatalf("record frame %d = %+v", position, frames[position])
		}
	}
	end := frames[3]
	if end.Type != records.JSONLFrameEnd || end.Complete == nil || !*end.Complete || end.Truncated == nil || *end.Truncated || end.Count == nil || *end.Count != 2 {
		t.Fatalf("end = %+v", end)
	}
	if len(end.Warnings) != 1 || end.Warnings[0] != "records.wago_cache_hit" {
		t.Fatalf("end warnings = %v", end.Warnings)
	}
}

func TestWriteJSONLFrameValidation(t *testing.T) {
	var buffer bytes.Buffer
	complete, truncated := true, false
	count := 0
	valid := map[string]records.JSONLFrame{
		"begin":  {Type: records.JSONLFrameBegin, Context: json.RawMessage(`{"a":1}`)},
		"record": {Type: records.JSONLFrameRecord, Index: &count, Record: json.RawMessage(`{}`)},
		"end":    {Type: records.JSONLFrameEnd, Complete: &complete, Truncated: &truncated, Count: &count},
		"error":  {Type: records.JSONLFrameError, Error: &records.JSONLError{Code: "records.hotfix_wago_http", Message: "boom", Stage: "fetch"}},
	}
	for name, frame := range valid {
		if err := records.WriteJSONLFrame(&buffer, frame); err != nil {
			t.Fatalf("%s: %v", name, err)
		}
	}
	invalid := map[string]records.JSONLFrame{
		"unknown type":   {Type: "middle"},
		"begin w/o ctx":  {Type: records.JSONLFrameBegin},
		"record w/o rec": {Type: records.JSONLFrameRecord, Index: &count},
		"end w/o count":  {Type: records.JSONLFrameEnd, Complete: &complete, Truncated: &truncated},
		"error w/o code": {Type: records.JSONLFrameError, Error: &records.JSONLError{}},
	}
	for name, frame := range invalid {
		if err := records.WriteJSONLFrame(&buffer, frame); err == nil {
			t.Fatalf("%s: accepted", name)
		}
	}
}

func TestWriteJSONLErrorFrame(t *testing.T) {
	var buffer bytes.Buffer
	if err := records.WriteJSONLErrorFrame(&buffer, "PIN-2", records.JSONLError{Code: "records.hotfix_offline", Message: "page 2 is not in the verified cache", Stage: "fetch", Retryable: true}); err != nil {
		t.Fatal(err)
	}
	frames := decodeJSONLFrames(t, buffer.String())
	if len(frames) != 1 || frames[0].Type != records.JSONLFrameError || frames[0].Error == nil || !frames[0].Error.Retryable {
		t.Fatalf("frame = %+v", frames[0])
	}
	// An error frame is not an end frame: consumers can tell a failed stream
	// from a complete one.
	if frames[0].Complete != nil {
		t.Fatalf("error frame claims completion: %+v", frames[0])
	}
}
