//go:build !linux

package execution

import (
	"context"
	"errors"
)

func runPTY(ctx context.Context, o Options) (Result, error) {
	return Result{}, errors.New("PTY backend currently supports Linux only")
}
