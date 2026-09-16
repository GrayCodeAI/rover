//go:build linux

package execution

import (
	"context"
	"encoding/json"
	"net"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func TestPTYRoundtripAndDetach(t *testing.T) {
	root, e := os.MkdirTemp("", "rover-pty-")
	if e != nil {
		t.Fatal(e)
	}
	defer os.RemoveAll(root)
	socket := filepath.Join(root, "p.sock")
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	done := make(chan Result, 1)
	errs := make(chan error, 1)
	go func() {
		r, e := Run(ctx, Options{Dir: root, OutputDir: filepath.Join(root, "out"), Argv: []string{"/bin/sh", "-c", "test -t 0 && printf READY; read x; printf 'RESULT:%s\\n' \"$x\"; read y; printf 'FINAL:%s\\n' \"$y\""}, Mode: "local-advisory", Timeout: 4 * time.Second, Interactive: true, Socket: socket})
		done <- r
		errs <- e
	}()
	var c net.Conn
	for deadline := time.Now().Add(2 * time.Second); time.Now().Before(deadline); {
		c, e = net.Dial("unix", socket)
		if e == nil {
			break
		}
		time.Sleep(10 * time.Millisecond)
	}
	if e != nil {
		t.Fatal(e)
	}
	enc := json.NewEncoder(c)
	_ = enc.Encode(terminalFrame{Type: "input", Data: []byte("one\n")})
	_ = c.SetReadDeadline(time.Now().Add(time.Second))
	dec := json.NewDecoder(c)
	text := ""
	for !strings.Contains(text, "RESULT:one") {
		var f terminalFrame
		if e = dec.Decode(&f); e != nil {
			t.Fatal(e)
		}
		text += string(f.Data)
	}
	_ = enc.Encode(terminalFrame{Type: "detach"})
	c.Close()
	time.Sleep(40 * time.Millisecond)
	c, e = net.Dial("unix", socket)
	if e != nil {
		t.Fatal(e)
	}
	_ = c.SetDeadline(time.Now().Add(time.Second))
	enc = json.NewEncoder(c)
	_ = enc.Encode(terminalFrame{Type: "resize", Rows: 40, Cols: 120})
	_ = enc.Encode(terminalFrame{Type: "input", Data: []byte("two\n")})
	dec = json.NewDecoder(c)
	for {
		var f terminalFrame
		if dec.Decode(&f) != nil {
			break
		}
		text += string(f.Data)
	}
	c.Close()
	r := <-done
	if e = <-errs; e != nil {
		t.Fatal(e)
	}
	if r.Process.ExitCode != 0 || !strings.Contains(string(r.Stdout), "FINAL:two") || !strings.Contains(text, "FINAL:two") {
		t.Fatalf("r=%+v stream=%q capture=%q", r.Process, text, r.Stdout)
	}
}
func TestPTYCancel(t *testing.T) {
	root, e := os.MkdirTemp("", "rpty-")
	if e != nil {
		t.Fatal(e)
	}
	defer os.RemoveAll(root)
	r, e := Run(context.Background(), Options{Dir: root, OutputDir: filepath.Join(root, "o"), Argv: []string{"/bin/sh", "-c", "sleep 20"}, Mode: "local-advisory", Timeout: 80 * time.Millisecond, Interactive: true, Socket: filepath.Join(root, "s")})
	if e != nil || !r.Process.TimedOut {
		t.Fatalf("%+v %v", r.Process, e)
	}
}
