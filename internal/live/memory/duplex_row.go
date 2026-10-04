package memory

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"errors"
	"io"
	"math"

	"github.com/follenfang/lycheedev/internal/live/duplex"
)

const duplexValueBytes = 24
const duplexCommandWords = (duplex.HeaderBytes + duplex.MaxSourceBytes) / 4

// A range fact describes the one attempted publication, not individual cells.
// It is buffered until the memory operation has ended; the caller persists it.
type DuplexWriteRange struct {
	Phase         string `json:"phase"`
	ExpectedBytes uint64 `json:"expectedBytes,string"`
	Bytes         uint64 `json:"bytes,string"`
	ImageSHA256   string `json:"imageSHA256"`
	Error         string `json:"error,omitempty"`
}

type duplexRow struct {
	a       *luaAccess
	address uint64
	image   []byte
	frozen  bool
}

type duplexRowPlan struct {
	image []byte
	fact  DuplexWriteRange
}

func duplexMessageRow(m duplex.Message) ([]DuplexPath, int) {
	if m.Header.Kind == duplex.Frame {
		return []DuplexPath{{Name: "inbox"}, {Name: "command"}}, duplexCommandWords
	}
	return []DuplexPath{{Name: "inbox"}, {Name: "stop"}}, (duplex.HeaderBytes + duplex.MaxControlBytes) / 4
}

// This plan never changes a TValue's tag, secret flag or remaining metadata.
// It copies every logical record, including zero padding after a short command.
// A single WPM is not an atomic publication or an allocation lifetime pin.
func planDuplexRow(m duplex.Message, row *duplexRow) (duplexRowPlan, error) {
	var plan duplexRowPlan
	wire, err := duplex.EncodeMessage(m)
	if err != nil {
		return plan, err
	}
	_, words := duplexMessageRow(m)
	if row == nil || !row.frozen {
		return plan, mailboxError("duplex_row_not_frozen")
	}
	if len(row.image) != words*duplexValueBytes {
		return plan, mailboxError("duplex_row_image_size")
	}
	plan.image = bytes.Clone(row.image)
	for i := 0; i < words; i++ {
		cell := row.image[i*duplexValueBytes : (i+1)*duplexValueBytes]
		value := math.Float64frombits(binary.LittleEndian.Uint64(cell))
		if cell[8] != 3 || cell[9] != 0 || math.IsNaN(value) || value < 0 || value > math.MaxUint32 || math.Trunc(value) != value {
			return duplexRowPlan{}, mailboxError("duplex_numeric_cell")
		}
		var packed [4]byte
		if start := i * 4; start < len(wire) {
			copy(packed[:], wire[start:min(start+4, len(wire))])
		}
		binary.LittleEndian.PutUint64(plan.image[i*duplexValueBytes:], math.Float64bits(float64(binary.LittleEndian.Uint32(packed[:]))))
		if !bytes.Equal(plan.image[i*duplexValueBytes+8:(i+1)*duplexValueBytes], cell[8:]) {
			return duplexRowPlan{}, mailboxError("duplex_nonpayload_changed")
		}
	}
	h := sha256.Sum256(plan.image)
	plan.fact = DuplexWriteRange{Phase: "whole_row", ExpectedBytes: uint64(len(plan.image)), ImageSHA256: hex.EncodeToString(h[:])}
	return plan, nil
}

// Only the Windows typed publisher supplies these operations. Keeping this
// executor private prevents an arbitrary address/raw-byte write API.
func executeDuplexRow(ctx context.Context, plan duplexRowPlan, before func(context.Context) error, write func([]byte) (int, error), read func(context.Context, []byte) (int, error), after func(context.Context) error) (out duplex.WriteOutcome, fact DuplexWriteRange, err error) {
	out.State = duplex.NoWrite
	fact = plan.fact
	if _, bounded := ctx.Deadline(); !bounded || before == nil || write == nil || read == nil || after == nil || len(plan.image) == 0 {
		return out, fact, errors.New("memory.duplex_row_configuration")
	}
	if err = before(ctx); err != nil {
		return out, fact, err
	}
	if err = ctx.Err(); err != nil {
		return out, fact, err
	}
	// Once the syscall is entered, a zero-byte failure is conservatively unknown.
	// It cannot justify replaying the original command.
	out.State = duplex.UnknownWrite
	n, writeErr := write(plan.image)
	if n > 0 {
		out.Bytes = uint64(n)
		fact.Bytes = uint64(n)
		out.State = duplex.PartialWrite
	}
	if writeErr != nil || n != len(plan.image) {
		err = errors.Join(writeErr, io.ErrShortWrite)
		fact.Error = err.Error()
		return out, fact, err
	}
	check := make([]byte, len(plan.image))
	n, err = read(ctx, check)
	if err != nil || n != len(check) {
		err = errors.Join(err, io.ErrUnexpectedEOF)
	} else if !bytes.Equal(check, plan.image) {
		err = errors.New("live.duplex_readback_mismatch")
	} else {
		err = after(ctx)
	}
	if err != nil {
		fact.Error = err.Error()
		return out, fact, err
	}
	out.State, out.ReadbackVerified = duplex.CompleteWrite, true
	return out, fact, nil
}
