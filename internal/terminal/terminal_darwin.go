//go:build darwin

package terminal

/*
#include <termios.h>
#include <unistd.h>
*/
import "C"
import (
	"context"
	"errors"
	"os"
	"syscall"
	"time"
	"unsafe"
)

const (
	tiocgwinsz = 0x40087468
	tiocswinsz = 0x80087467
)

func IsTTY(f *os.File) bool {
	return C.isatty(C.int(f.Fd())) == 1
}

func Raw(f *os.File) (func() error, error) {
	var old C.struct_termios
	if C.tcgetattr(C.int(f.Fd()), &old) != 0 {
		return nil, errors.New("tcgetattr failed")
	}
	raw := old
	C.cfmakeraw(&raw)
	// Match Linux Raw: VMIN 0 VTIME 1 (poll with 100ms timeout) so Read can
	// be interrupted via context without blocking forever.
	raw.c_cc[16] = 0 // VMIN
	raw.c_cc[17] = 1 // VTIME
	if C.tcsetattr(C.int(f.Fd()), C.TCSANOW, &raw) != 0 {
		return nil, errors.New("tcsetattr failed")
	}
	return func() error {
		if C.tcsetattr(C.int(f.Fd()), C.TCSANOW, &old) != 0 {
			return errors.New("tcsetattr restore failed")
		}
		return nil
	}, nil
}

type Dimensions struct{ Rows, Cols uint16 }

func Size(f *os.File) Dimensions {
	var w [4]uint16
	_, _, e := syscall.Syscall(syscall.SYS_IOCTL, f.Fd(), uintptr(tiocgwinsz), uintptr(unsafe.Pointer(&w)))
	if e != 0 || w[0] == 0 || w[1] == 0 {
		return Dimensions{24, 80}
	}
	return Dimensions{w[0], w[1]}
}

func Read(ctx context.Context, f *os.File, p []byte) (int, error) {
	for {
		if e := ctx.Err(); e != nil {
			return 0, e
		}
		n, e := syscall.Read(int(f.Fd()), p)
		if e == syscall.EINTR {
			continue
		}
		if n > 0 || e != nil {
			return n, e
		}
		select {
		case <-ctx.Done():
			return 0, ctx.Err()
		case <-time.After(10 * time.Millisecond):
		}
	}
}

func Available() bool { return true }

// ensure tiocswinsz is referenced (used by execution/pty_darwin.go via same constant)
var _ = tiocswinsz
