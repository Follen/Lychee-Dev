package main

import (
	"context"
	"github.com/follenfang/lycheedev/internal/command"
	"github.com/follenfang/lycheedev/internal/live/memory"
	"os"
	"os/signal"
)

func main() {
	if handled, code := memory.RunMailboxMemoryHelper(os.Args[1:], os.Stdin, os.Stdout); handled {
		os.Exit(code)
	}
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt)
	code := command.Execute(ctx, os.Args[1:], os.Stdout, os.Stderr)
	stop()
	os.Exit(code)
}
