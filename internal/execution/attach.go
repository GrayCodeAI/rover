package execution

import (
	"bufio"
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"github.com/GrayCodeAI/rover/internal/terminal"
	"net"
	"os"
	"sync"
	"time"
)

// Attach is deliberately interactive: terminal output is not a safe rendering
// of hostile data. Use trusted local sessions; reports remain sanitized.
func Attach(parent context.Context, socket string, input, output *os.File) error {
	if !terminal.IsTTY(input) || !terminal.IsTTY(output) {
		return errors.New("attach requires real input/output terminals")
	}
	c, e := net.DialTimeout("unix", socket, 2*time.Second)
	if e != nil {
		return e
	}
	defer c.Close()
	restore, e := terminal.Raw(input)
	if e != nil {
		return e
	}
	defer restore()
	ctx, cancel := context.WithCancel(parent)
	defer cancel()
	var mu sync.Mutex
	enc := json.NewEncoder(c)
	send := func(v any) error {
		mu.Lock()
		defer mu.Unlock()
		_ = c.SetWriteDeadline(time.Now().Add(2 * time.Second))
		return enc.Encode(v)
	}
	size := terminal.Size(input)
	_ = send(map[string]any{"type": "resize", "rows": size.Rows, "cols": size.Cols})
	done := make(chan error, 1)
	go func() {
		sc := bufio.NewScanner(c)
		sc.Buffer(make([]byte, 4096), 128<<10)
		for sc.Scan() {
			var f struct {
				Type, Error string
				Data        []byte
			}
			if e := json.Unmarshal(sc.Bytes(), &f); e != nil {
				done <- e
				return
			}
			if f.Type == "error" {
				done <- errors.New(f.Error)
				return
			}
			if f.Type == "output" {
				if _, e := output.Write(f.Data); e != nil {
					done <- e
					return
				}
			}
		}
		done <- sc.Err()
	}()
	keys := make(chan []byte, 8)
	rd := make(chan struct{})
	go func() {
		defer close(rd)
		p := make([]byte, 4096)
		for {
			n, e := terminal.Read(ctx, input, p)
			if e != nil {
				return
			}
			select {
			case keys <- append([]byte(nil), p[:n]...):
			case <-ctx.Done():
				return
			}
		}
	}()
	defer func() { cancel(); <-rd }()
	tick := time.NewTicker(500 * time.Millisecond)
	defer tick.Stop()
	for {
		select {
		case <-ctx.Done():
			return nil
		case e := <-done:
			return e
		case p := <-keys:
			if i := bytes.IndexByte(p, 29); i >= 0 {
				if i > 0 {
					_ = send(map[string]any{"type": "input", "data": p[:i]})
				}
				_ = send(map[string]string{"type": "detach"})
				fmt.Fprint(output, "\r\nDetached; task continues.\r\n")
				return nil
			}
			if e := send(map[string]any{"type": "input", "data": p}); e != nil {
				return e
			}
		case <-tick.C:
			n := terminal.Size(input)
			if n != size {
				size = n
				_ = send(map[string]any{"type": "resize", "rows": size.Rows, "cols": size.Cols})
			}
		}
	}
}
