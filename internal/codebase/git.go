package codebase

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"strings"
	"time"
)

// This is a byte transport, not a shell. Disable inherited Git routing and
// credentials prompts, config includes from global/system files, replacements,
// external protocols and automatic maintenance. No checkout means no filters.
func gitBytes(ctx context.Context, directory string, limit int, arguments ...string) ([]byte, error) {
	return gitProbe(ctx, directory, limit, 5*time.Minute, arguments...)
}

// gitProbe is gitBytes with an explicit network/operation budget.
func gitProbe(ctx context.Context, directory string, limit int, timeout time.Duration, arguments ...string) ([]byte, error) {
	ctx, cancel := context.WithTimeout(ctx, timeout)
	defer cancel()
	cmd := gitCommand(ctx, directory, arguments...)
	out := &boundedOutput{limit: limit, cancel: cancel}
	log := &boundedOutput{limit: 32768, cancel: cancel}
	cmd.Stdout, cmd.Stderr = out, log
	err := cmd.Run()
	if out.exceeded || log.exceeded {
		return nil, errors.New("codebase.git_output_limit")
	}
	if ctx.Err() != nil {
		return nil, ctx.Err()
	}
	if err != nil {
		return nil, fmt.Errorf("codebase.git_failed: %w: %s", err, strings.TrimSpace(log.String()))
	}
	return out.buffer.Bytes(), nil
}

func gitCommand(ctx context.Context, directory string, arguments ...string) *exec.Cmd {
	args := []string{"-c", "core.hooksPath=" + os.DevNull, "-c", "protocol.allow=never", "-c", "protocol.https.allow=always", "-c", "maintenance.auto=false", "-c", "gc.auto=0"}
	if directory != "" {
		args = append(args, "--git-dir="+directory)
	}
	cmd := exec.CommandContext(ctx, "git", append(args, arguments...)...)
	for _, entry := range os.Environ() {
		key, _, _ := strings.Cut(entry, "=")
		if !strings.HasPrefix(strings.ToUpper(key), "GIT_") && !strings.EqualFold(key, "GCM_INTERACTIVE") {
			cmd.Env = append(cmd.Env, entry)
		}
	}
	cmd.Env = append(cmd.Env, "GIT_CONFIG_NOSYSTEM=1", "GIT_CONFIG_GLOBAL="+os.DevNull, "GIT_TERMINAL_PROMPT=0", "GCM_INTERACTIVE=never", "GIT_NO_REPLACE_OBJECTS=1")
	cmd.WaitDelay = 3 * time.Second
	return cmd
}

type boundedOutput struct {
	buffer   bytes.Buffer
	limit    int
	cancel   context.CancelFunc
	exceeded bool
}

func (b *boundedOutput) String() string { return b.buffer.String() }
func (b *boundedOutput) Write(data []byte) (int, error) {
	if len(data) > b.limit-b.buffer.Len() {
		b.exceeded = true
		b.cancel()
		return 0, errors.New("output limit")
	}
	return b.buffer.Write(data)
}
