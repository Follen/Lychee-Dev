package main

import (
	"context"
	"github.com/follenfang/lycheedev/internal/command"
	"github.com/follenfang/lycheedev/internal/publish"
	"os"
	"os/signal"
)

func main() {
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt)
	if handled, code := publish.HandleDDSessionWorker(ctx, os.Args[1:]); handled {
		stop()
		os.Exit(code)
	}
	code := command.Execute(ctx, os.Args[1:], os.Stdout, os.Stderr)
	stop()
	os.Exit(code)
}
