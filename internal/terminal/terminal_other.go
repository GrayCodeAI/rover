//go:build !linux

package terminal

import (
	"context"
	"errors"
	"os"
)

type Dimensions struct{ Rows, Cols uint16 }

func Available() bool            { return false }
func IsTTY(f *os.File) bool      { return false }
func Size(f *os.File) Dimensions { return Dimensions{24, 80} }
func Raw(f *os.File) (func() error, error) {
	return nil, errors.New("interactive terminal currently supports Linux")
}
func Read(ctx context.Context, f *os.File, p []byte) (int, error) {
	return 0, errors.New("terminal unsupported")
}
