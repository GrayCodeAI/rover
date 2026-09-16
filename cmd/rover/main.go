// Rover is a local-first agent runtime and evidence CLI.
package main

import (
	"context"
	"github.com/GrayCodeAI/rover/internal/cli"
	"os"
	"os/signal"
	"syscall"
)

func main() {
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	code := cli.New(os.Stdout, os.Stderr).Main(ctx, os.Args[1:])
	stop()
	os.Exit(code)
}
