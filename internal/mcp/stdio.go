package mcp

import (
	"bufio"
	"context"
	"encoding/json"
	"github.com/GrayCodeAI/rover/internal/service"
	"io"
	"sync"
)

func ServeStdio(parent context.Context, s *service.Service, in io.Reader, out io.Writer) error {
	ctx, cancel := context.WithCancel(parent)
	defer cancel()
	var writer sync.Mutex
	send := func(r Response) {
		writer.Lock()
		defer writer.Unlock()
		if json.NewEncoder(out).Encode(r) != nil {
			cancel()
		}
	}
	type line struct {
		b []byte
		e error
	}
	lines := make(chan line, 8)
	go func() {
		defer close(lines)
		sc := bufio.NewScanner(in)
		sc.Buffer(make([]byte, 4096), MaxMessage)
		for sc.Scan() {
			select {
			case lines <- line{b: append([]byte(nil), sc.Bytes()...)}:
			case <-ctx.Done():
				return
			}
		}
		if e := sc.Err(); e != nil {
			select {
			case lines <- line{e: e}:
			case <-ctx.Done():
			}
		}
	}()
	var activeMu sync.Mutex
	active := map[string]context.CancelFunc{}
	sem := make(chan struct{}, 8)
	var wg sync.WaitGroup
	defer func() {
		cancel()
		if c, ok := in.(io.Closer); ok {
			c.Close()
		}
		if c, ok := out.(io.Closer); ok {
			c.Close()
		}
		activeMu.Lock()
		for _, f := range active {
			f()
		}
		activeMu.Unlock()
		wg.Wait()
	}()
	started, initialized := false, false
	for {
		select {
		case <-ctx.Done():
			return ctx.Err()
		case l, ok := <-lines:
			if !ok {
				return nil
			}
			if l.e != nil {
				send(failure(nil, -32700, "input exceeds framing limit or read failed"))
				return l.e
			}
			r, e := decode(l.b)
			if e != nil {
				send(failure(nil, -32700, errText(e)))
				continue
			}
			if len(r.ID) == 0 {
				switch r.Method {
				case "notifications/initialized":
					if started {
						initialized = true
					}
				case "notifications/cancelled":
					key, e := cancelledID(r)
					if e == nil {
						activeMu.Lock()
						f := active[key]
						activeMu.Unlock()
						if f != nil {
							f()
						}
					}
				}
				continue
			}
			if r.Method == "initialize" {
				if started {
					send(failure(r.ID, -32600, "already initialized"))
					continue
				}
				resp := handle(ctx, s, r, nil, false)
				if resp.Error == nil {
					started = true
				}
				send(resp)
				continue
			}
			if !initialized && r.Method != "ping" {
				send(failure(r.ID, -32002, "initialization incomplete"))
				continue
			}
			key, _ := idKey(r.ID)
			activeMu.Lock()
			if _, exists := active[key]; exists {
				activeMu.Unlock()
				send(failure(r.ID, -32600, "duplicate in-flight request id"))
				continue
			}
			cctx, stop := context.WithCancel(ctx)
			active[key] = stop
			activeMu.Unlock()
			select {
			case sem <- struct{}{}:
			default:
				stop()
				activeMu.Lock()
				delete(active, key)
				activeMu.Unlock()
				send(failure(r.ID, -32000, "concurrency budget exceeded"))
				continue
			}
			wg.Add(1)
			go func(r Request, key string) {
				defer wg.Done()
				defer func() { stop(); <-sem; activeMu.Lock(); delete(active, key); activeMu.Unlock() }()
				send(handle(cctx, s, r, nil, true))
			}(r, key)
		}
	}
}
