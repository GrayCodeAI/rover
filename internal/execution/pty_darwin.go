//go:build darwin

package execution

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"io"
	"net"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"sync"
	"syscall"
	"time"
	"unsafe"

	"github.com/GrayCodeAI/rover/internal/model"
	"github.com/GrayCodeAI/rover/internal/store"
)

// tiocswinszDarwin sets the PTY window size on macOS.
const tiocswinszDarwin = 0x80087467

// macOS PTY ioctl constants (from sys/ttycom.h). These are not exposed in Go's
// standard syscall package for darwin, so we define them with raw values.
const (
	// TIOCPTYGRANT = _IO('t', 84) — grant permissions to the slave PTY
	tiocptygrant = 0x20007454
	// TIOCPTYUNLK = _IO('t', 82) — unlock the slave PTY
	tiocptyunlk = 0x20007452
	// TIOCPTYGNAME = _IOC(IOC_OUT, 't', 83, 128) — get slave PTY path name
	tiocptygname = 0x40807453
)

type terminalFrame struct {
	Type  string `json:"type"`
	Data  []byte `json:"data,omitempty"`
	Rows  uint16 `json:"rows,omitempty"`
	Cols  uint16 `json:"cols,omitempty"`
	Error string `json:"error,omitempty"`
}

type broker struct {
	mu      sync.Mutex
	ln      net.Listener
	path    string
	master  *os.File
	ring    []byte
	client  net.Conn
	ch      chan terminalFrame
	closed  bool
	drained chan struct{}
}

func ptyIoctl(fd, req uintptr, p unsafe.Pointer) error {
	_, _, e := syscall.Syscall(syscall.SYS_IOCTL, fd, req, uintptr(p))
	if e != 0 {
		return e
	}
	return nil
}

func openPTY() (*os.File, *os.File, error) {
	m, e := os.OpenFile("/dev/ptmx", os.O_RDWR|syscall.O_NOCTTY, 0600)
	if e != nil {
		return nil, nil, e
	}
	// grantpt — grant permissions to the slave PTY
	if e := ptyIoctl(m.Fd(), uintptr(tiocptygrant), nil); e != nil {
		m.Close()
		return nil, nil, e
	}
	// unlockpt — unlock the slave PTY
	if e := ptyIoctl(m.Fd(), uintptr(tiocptyunlk), nil); e != nil {
		m.Close()
		return nil, nil, e
	}
	// ptsname — get the slave PTY path name
	var buf [128]byte
	if e := ptyIoctl(m.Fd(), uintptr(tiocptygname), unsafe.Pointer(&buf[0])); e != nil {
		m.Close()
		return nil, nil, e
	}
	slavePath := strings.TrimRight(string(buf[:]), "\x00")
	if slavePath == "" {
		m.Close()
		return nil, nil, errors.New("empty PTY slave path")
	}
	sl, e := os.OpenFile(slavePath, os.O_RDWR|syscall.O_NOCTTY, 0600)
	if e != nil {
		m.Close()
		return nil, nil, e
	}
	w := [4]uint16{24, 80}
	_ = ptyIoctl(m.Fd(), uintptr(tiocswinszDarwin), unsafe.Pointer(&w))
	return m, sl, nil
}

func startBroker(path string, m *os.File) (*broker, error) {
	if len(path) > 103 {
		return nil, errors.New("PTY socket path exceeds platform limit; choose shorter --state")
	}
	if e := store.PrivateDir(filepath.Dir(path)); e != nil {
		return nil, e
	}
	if _, e := os.Lstat(path); !os.IsNotExist(e) {
		return nil, errors.New("PTY socket already exists; takeover is not automatic")
	}
	ln, e := net.Listen("unix", path)
	if e != nil {
		return nil, e
	}
	if e = os.Chmod(path, 0600); e != nil {
		ln.Close()
		return nil, e
	}
	b := &broker{ln: ln, master: m, path: path}
	go b.accept()
	return b, nil
}

func (b *broker) accept() {
	for {
		c, e := b.ln.Accept()
		if e != nil {
			return
		}
		b.mu.Lock()
		if b.closed || b.client != nil {
			b.mu.Unlock()
			_ = c.SetWriteDeadline(time.Now().Add(time.Second))
			_ = json.NewEncoder(c).Encode(terminalFrame{Type: "error", Error: "session already has an attached writer"})
			c.Close()
			continue
		}
		b.client = c
		b.ch = make(chan terminalFrame, 64)
		b.drained = make(chan struct{})
		drained := b.drained
		ch := b.ch
		ring := append([]byte(nil), b.ring...)
		b.mu.Unlock()
		go b.serve(c, ch, ring, drained)
	}
}

func (b *broker) serve(c net.Conn, ch chan terminalFrame, ring []byte, drained chan struct{}) {
	defer func() {
		c.Close()
		b.mu.Lock()
		if b.client == c {
			b.client = nil
			b.ch = nil
		}
		b.mu.Unlock()
	}()
	done := make(chan struct{})
	defer close(done)
	go func() {
		defer close(drained)
		defer c.Close()
		enc := json.NewEncoder(c)
		write := func(f terminalFrame) error {
			_ = c.SetWriteDeadline(time.Now().Add(2 * time.Second))
			return enc.Encode(f)
		}
		if write(terminalFrame{Type: "output", Data: ring}) != nil {
			c.Close()
			return
		}
		for {
			select {
			case <-done:
				return
			case f := <-ch:
				if write(f) != nil || f.Type == "exit" {
					c.Close()
					return
				}
			}
		}
	}()
	sc := bufio.NewScanner(c)
	sc.Buffer(make([]byte, 4096), 64<<10)
	for sc.Scan() {
		var f terminalFrame
		if json.Unmarshal(sc.Bytes(), &f) != nil {
			return
		}
		switch f.Type {
		case "input":
			if len(f.Data) > 16<<10 {
				return
			}
			if _, e := b.master.Write(f.Data); e != nil {
				return
			}
		case "resize":
			if f.Rows == 0 || f.Cols == 0 || f.Rows > 1000 || f.Cols > 1000 {
				return
			}
			w := [4]uint16{f.Rows, f.Cols}
			if ptyIoctl(b.master.Fd(), tiocswinszDarwin, unsafe.Pointer(&w)) != nil {
				return
			}
		case "detach":
			return
		default:
			return
		}
	}
}

func (b *broker) Write(p []byte) (int, error) {
	b.mu.Lock()
	defer b.mu.Unlock()
	n := len(p)
	b.ring = append(b.ring, p...)
	if len(b.ring) > 32<<10 {
		b.ring = append([]byte(nil), b.ring[len(b.ring)-(32<<10):]...)
	}
	if b.ch != nil {
		select {
		case b.ch <- terminalFrame{Type: "output", Data: append([]byte(nil), p...)}:
		default:
			if b.client != nil {
				b.client.Close()
			}
		}
	}
	return n, nil
}

func (b *broker) Close() {
	b.mu.Lock()
	b.closed = true
	c, ch, drained := b.client, b.ch, b.drained
	if ch != nil {
		select {
		case ch <- terminalFrame{Type: "exit"}:
		default:
			c.Close()
		}
	}
	b.mu.Unlock()
	b.ln.Close()
	if b.path != "" {
		_ = os.Remove(b.path)
	}
	if c != nil {
		select {
		case <-drained:
		case <-time.After(250 * time.Millisecond):
		}
		c.Close()
	}
}

func runPTY(parent context.Context, o Options) (r Result, returned error) {
	if o.Mode != "local-advisory" || o.Socket == "" {
		return r, errors.New("PTY requires explicit local-advisory mode and a socket")
	}
	if e := store.PrivateDir(o.OutputDir); e != nil {
		return r, e
	}
	out, e := newCapture(filepath.Join(o.OutputDir, "stdout.log"))
	if e != nil {
		return r, e
	}
	defer func() {
		if e := out.close(); returned == nil {
			returned = e
		}
	}()
	er, e := newCapture(filepath.Join(o.OutputDir, "stderr.log"))
	if e != nil {
		return r, e
	}
	defer func() {
		if e := er.close(); returned == nil {
			returned = e
		}
	}()
	m, sl, e := openPTY()
	if e != nil {
		return r, e
	}
	defer m.Close()
	defer sl.Close()
	b, e := startBroker(o.Socket, m)
	if e != nil {
		return r, e
	}
	defer b.Close()
	env, e := cleanEnv(filepath.Join(o.OutputDir, "environment"), o.PassEnv)
	if e != nil {
		return r, e
	}
	env = append(env, "TERM=xterm-256color")
	ctx, cancel := context.WithTimeout(parent, o.Timeout)
	defer cancel()
	cmd := exec.CommandContext(ctx, o.Argv[0], o.Argv[1:]...)
	cmd.Dir = o.Dir
	cmd.Env = env
	cmd.Stdin = sl
	cmd.Stdout = sl
	cmd.Stderr = sl
	cmd.SysProcAttr = &syscall.SysProcAttr{Setsid: true, Setctty: true, Ctty: 0}
	cmd.Cancel = func() error {
		if cmd.Process == nil {
			return os.ErrProcessDone
		}
		e := syscall.Kill(-cmd.Process.Pid, syscall.SIGKILL)
		if e == syscall.ESRCH {
			return os.ErrProcessDone
		}
		return e
	}
	r.Executable = cmd.Path
	r.Process.StartedAt = model.Now()
	r.Process.ExitCode = -1
	if e = cmd.Start(); e != nil {
		return r, e
	}
	sl.Close()
	rd := make(chan error, 1)
	go func() { _, e := io.Copy(io.MultiWriter(out, b), m); rd <- e }()
	e = cmd.Wait()
	if ctx.Err() != nil || parent.Err() != nil {
		cleanupProcess(cmd)
	}
	var captureErr error
	select {
	case captureErr = <-rd:
	case <-time.After(time.Second):
		m.Close()
		captureErr = <-rd
	}
	r.Process.FinishedAt = model.Now()
	r.Process.ExitCode = cmd.ProcessState.ExitCode()
	if e != nil {
		r.Process.Error = e.Error()
	}
	if captureErr != nil && !errors.Is(captureErr, syscall.EIO) && !errors.Is(captureErr, os.ErrClosed) {
		r.Process.Error += "; PTY capture: " + captureErr.Error()
	}
	r.Process.TimedOut = errors.Is(ctx.Err(), context.DeadlineExceeded)
	r.Process.Cancelled = parent.Err() != nil
	r.Stdout = append([]byte(nil), out.b...)
	r.Stderr = []byte{}
	r.Process.StdoutSHA256 = model.Digest(r.Stdout)
	r.Process.StderrSHA256 = model.Digest(nil)
	r.Process.StdoutBytes = out.seen
	r.Process.Truncated = out.truncated
	return r, nil
}
