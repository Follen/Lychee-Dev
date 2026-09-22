// SPDX-License-Identifier: AGPL-3.0-or-later
package records

import (
	"encoding/json"
	"errors"
	"fmt"
	"io"

	"github.com/follenfang/lycheedev/internal/selection"
)

// JSONLSchema versions the typed begin/record/end/error stream frames from the
// unified result rules (design section 7). Only a legal end frame followed by
// exit 0 is a complete success; a broken pipe never fakes completion.
const JSONLSchema = "lycheedev.jsonl.v1"

const (
	JSONLFrameBegin  = "begin"
	JSONLFrameRecord = "record"
	JSONLFrameEnd    = "end"
	JSONLFrameError  = "error"
)

var ErrJSONLFrame = errors.New("records.jsonl_frame")

// JSONLError is the constrained error payload of an error frame.
type JSONLError struct {
	Code              string `json:"code"`
	Message           string `json:"message"`
	Stage             string `json:"stage"`
	Retryable         bool   `json:"retryable"`
	ResumeOperationID string `json:"resumeOperationId,omitempty"`
}

// JSONLFrame is one typed stream frame. Context belongs to begin, Index and
// Record to record, Complete/Truncated/Count to end and Error to error frames.
type JSONLFrame struct {
	Schema      string          `json:"schema"`
	Type        string          `json:"type"`
	OperationID string          `json:"operationId,omitempty"`
	Context     json.RawMessage `json:"context,omitempty"`
	Index       *int            `json:"index,omitempty"`
	Record      json.RawMessage `json:"record,omitempty"`
	Complete    *bool           `json:"complete,omitempty"`
	Truncated   *bool           `json:"truncated,omitempty"`
	Count       *int            `json:"count,omitempty"`
	Warnings    []string        `json:"warnings,omitempty"`
	Error       *JSONLError     `json:"error,omitempty"`
}

// WriteJSONLFrame validates and writes exactly one frame line.
func WriteJSONLFrame(w io.Writer, frame JSONLFrame) error {
	frame.Schema = JSONLSchema
	switch frame.Type {
	case JSONLFrameBegin:
		if len(frame.Context) == 0 {
			return fmt.Errorf("%w: begin frame needs context", ErrJSONLFrame)
		}
	case JSONLFrameRecord:
		if len(frame.Record) == 0 || frame.Index == nil {
			return fmt.Errorf("%w: record frame needs an index and record", ErrJSONLFrame)
		}
	case JSONLFrameEnd:
		if frame.Complete == nil || frame.Truncated == nil || frame.Count == nil {
			return fmt.Errorf("%w: end frame needs complete, truncated and count", ErrJSONLFrame)
		}
	case JSONLFrameError:
		if frame.Error == nil || frame.Error.Code == "" {
			return fmt.Errorf("%w: error frame needs an error code", ErrJSONLFrame)
		}
	default:
		return fmt.Errorf("%w: unknown frame type %q", ErrJSONLFrame, frame.Type)
	}
	raw, err := json.Marshal(frame)
	if err != nil {
		return err
	}
	_, err = w.Write(append(raw, '\n'))
	return err
}

// WriteHotfixJSONL streams one hotfix result as typed frames: a begin frame
// carrying the fixed query context, one record frame per hotfix record and an
// end frame carrying completeness. The end frame is written only after every
// record frame succeeded.
func WriteHotfixJSONL(w io.Writer, result RemoteHotfixResult) error {
	context, err := json.Marshal(struct {
		Query    HotfixQuery         `json:"query"`
		Coverage HotfixCoverage      `json:"coverage"`
		Change   selection.ChangePin `json:"change"`
	}{result.Query, result.Coverage, result.Change})
	if err != nil {
		return err
	}
	operation := result.Snapshot
	if err := WriteJSONLFrame(w, JSONLFrame{Type: JSONLFrameBegin, OperationID: operation, Context: context}); err != nil {
		return err
	}
	for index := range result.Records {
		record, err := json.Marshal(result.Records[index])
		if err != nil {
			return err
		}
		position := index
		if err := WriteJSONLFrame(w, JSONLFrame{Type: JSONLFrameRecord, OperationID: operation, Index: &position, Record: record}); err != nil {
			return err
		}
	}
	count := len(result.Records)
	return WriteJSONLFrame(w, JSONLFrame{
		Type:        JSONLFrameEnd,
		OperationID: operation,
		Complete:    &result.Complete,
		Truncated:   &result.Truncated,
		Count:       &count,
		Warnings:    result.Warnings,
	})
}

// WriteJSONLErrorFrame ends a failed stream explicitly instead of pretending
// completion with an end frame.
func WriteJSONLErrorFrame(w io.Writer, operationID string, failure JSONLError) error {
	return WriteJSONLFrame(w, JSONLFrame{Type: JSONLFrameError, OperationID: operationID, Error: &failure})
}
