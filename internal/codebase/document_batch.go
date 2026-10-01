package codebase

import (
	"bufio"
	"context"
	"crypto/sha1"
	"encoding/hex"
	"errors"
	"fmt"
	"io"
	"os/exec"
	"time"

	"github.com/follenfang/lycheedev/internal/selection"
)

type documentBatch struct {
	cmd    *exec.Cmd
	input  io.WriteCloser
	output io.ReadCloser
	reader *bufio.Reader
	cancel context.CancelFunc
	ctx    context.Context
}

func newDocumentBatch(ctx context.Context, b *Browser, pin selection.SourcePin) (*documentBatch, error) {
	ctx, cancel := context.WithTimeout(ctx, 2*time.Minute)
	cmd := gitCommand(ctx, b.mirror(pin.Repository), "cat-file", "--batch")
	in, err := cmd.StdinPipe()
	if err != nil {
		cancel()
		return nil, err
	}
	out, err := cmd.StdoutPipe()
	if err != nil {
		in.Close()
		cancel()
		return nil, err
	}
	cmd.Stderr = &boundedOutput{limit: 32 << 10, cancel: cancel}
	if err := cmd.Start(); err != nil {
		in.Close()
		out.Close()
		cancel()
		return nil, err
	}
	return &documentBatch{cmd: cmd, input: in, output: out, reader: bufio.NewReaderSize(out, 64<<10), cancel: cancel, ctx: ctx}, nil
}
func (b *documentBatch) read(object string, size int64) (data []byte, err error) {
	defer func() {
		if canceled := b.ctx.Err(); canceled != nil {
			data = nil
			err = canceled
		}
	}()
	if err := b.ctx.Err(); err != nil {
		return nil, err
	}
	if _, err := io.WriteString(b.input, object+"\n"); err != nil {
		return nil, err
	}
	header, err := b.reader.ReadSlice('\n')
	if err != nil {
		return nil, err
	}
	if string(header) != fmt.Sprintf("%s blob %d\n", object, size) {
		return nil, errors.New("codebase.batch_identity_mismatch")
	}
	data = make([]byte, int(size)+1)
	if _, err := io.ReadFull(b.reader, data); err != nil {
		return nil, err
	}
	if data[len(data)-1] != '\n' {
		return nil, errors.New("codebase.batch_framing")
	}
	data = data[:len(data)-1]
	h := sha1.New()
	fmt.Fprintf(h, "blob %d\x00", len(data))
	h.Write(data)
	if hex.EncodeToString(h.Sum(nil)) != object {
		return nil, errors.New("codebase.git_blob_integrity")
	}
	return data, nil
}
func (b *documentBatch) close() { b.input.Close(); b.cancel(); b.output.Close(); _ = b.cmd.Wait() }
