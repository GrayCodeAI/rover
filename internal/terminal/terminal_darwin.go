//go:build darwin

package terminal

import (
	"context"
	"os"
	"syscall"
	"time"
	"unsafe"
)

func ioctl(fd, req uintptr, p unsafe.Pointer) error {
	_, _, e := syscall.Syscall(syscall.SYS_IOCTL, fd, req, uintptr(p))
	if e != 0 {
		return e
	}
	return nil
}

func IsTTY(f *os.File) bool {
	var t syscall.Termios
	return ioctl(f.Fd(), syscall.TIOCGETA, unsafe.Pointer(&t)) == nil
}

func Raw(f *os.File) (func() error, error) {
	var old syscall.Termios
	if e := ioctl(f.Fd(), syscall.TIOCGETA, unsafe.Pointer(&old)); e != nil {
		return nil, e
	}
	n := old
	n.Iflag &^= syscall.IGNBRK | syscall.BRKINT | syscall.PARMRK | syscall.ISTRIP | syscall.INLCR | syscall.IGNCR | syscall.ICRNL | syscall.IXON
	n.Oflag &^= syscall.OPOST
	n.Lflag &^= syscall.ECHO | syscall.ECHONL | syscall.ICANON | syscall.ISIG | syscall.IEXTEN
	n.Cflag &^= syscall.CSIZE | syscall.PARENB
	n.Cflag |= syscall.CS8
	n.Cc[syscall.VMIN] = 0
	n.Cc[syscall.VTIME] = 1
	if e := ioctl(f.Fd(), syscall.TIOCSETA, unsafe.Pointer(&n)); e != nil {
		return nil, e
	}
	return func() error { return ioctl(f.Fd(), syscall.TIOCSETA, unsafe.Pointer(&old)) }, nil
}

type Dimensions struct{ Rows, Cols uint16 }

func Size(f *os.File) Dimensions {
	var w [4]uint16
	if ioctl(f.Fd(), syscall.TIOCGWINSZ, unsafe.Pointer(&w)) != nil || w[0] == 0 || w[1] == 0 {
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
