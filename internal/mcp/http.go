package mcp

import (
	"context"
	"crypto/tls"
	"encoding/json"
	"errors"
	"github.com/GrayCodeAI/rover/internal/access"
	"github.com/GrayCodeAI/rover/internal/service"
	"io"
	"mime"
	"net"
	"net/http"
	"strings"
	"sync"
	"time"
)

func HTTP(s *service.Service) http.Handler {
	slots := make(chan struct{}, 8)
	var activeMu sync.Mutex
	active := map[string]context.CancelFunc{}
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("X-Content-Type-Options", "nosniff")
		w.Header().Set("Cache-Control", "no-store")
		if r.URL.Path != "/mcp" {
			http.NotFound(w, r)
			return
		}
		if r.Method != http.MethodPost {
			w.Header().Set("Allow", "POST")
			http.Error(w, "stateless endpoint supports POST only", http.StatusMethodNotAllowed)
			return
		}
		if r.Header.Get("Origin") != "" {
			http.Error(w, "browser origins are not authorized", http.StatusForbidden)
			return
		}
		auth := r.Header.Get("Authorization")
		if !strings.HasPrefix(auth, "Bearer ") {
			w.Header().Set("WWW-Authenticate", `Bearer realm="rover"`)
			http.Error(w, "bearer grant required", http.StatusUnauthorized)
			return
		}
		g, e := access.Authenticate(s.Store, s.Repository, strings.TrimPrefix(auth, "Bearer "))
		if e != nil {
			http.Error(w, "unauthorized", http.StatusUnauthorized)
			return
		}
		typ, _, e := mime.ParseMediaType(r.Header.Get("Content-Type"))
		if e != nil || typ != "application/json" {
			http.Error(w, "application/json required", http.StatusUnsupportedMediaType)
			return
		}
		r.Body = http.MaxBytesReader(w, r.Body, MaxMessage)
		b, e := io.ReadAll(r.Body)
		if e != nil {
			http.Error(w, "request exceeds message limit", http.StatusRequestEntityTooLarge)
			return
		}
		req, e := decode(b)
		if e != nil {
			w.Header().Set("Content-Type", "application/json")
			w.WriteHeader(http.StatusBadRequest)
			_ = json.NewEncoder(w).Encode(failure(nil, -32700, errText(e)))
			return
		}
		if req.Method != "initialize" && r.Header.Get("MCP-Protocol-Version") != Protocol {
			http.Error(w, "supported MCP-Protocol-Version header required", http.StatusBadRequest)
			return
		}
		if len(req.ID) == 0 {
			if req.Method != "notifications/initialized" && req.Method != "notifications/cancelled" {
				http.Error(w, "unsupported notification", http.StatusBadRequest)
				return
			}
			if req.Method == "notifications/cancelled" {
				key, e := cancelledID(req)
				if e != nil {
					http.Error(w, "invalid cancellation", http.StatusBadRequest)
					return
				}
				activeMu.Lock()
				f := active[g.ID+":"+key]
				activeMu.Unlock()
				if f != nil {
					f()
				}
			}
			w.WriteHeader(http.StatusAccepted)
			return
		}
		select {
		case slots <- struct{}{}:
			defer func() { <-slots }()
		default:
			http.Error(w, "concurrency limit", http.StatusTooManyRequests)
			return
		}
		expires, e := time.Parse(time.RFC3339Nano, g.ExpiresAt)
		if e != nil {
			http.Error(w, "grant expiry invalid", http.StatusUnauthorized)
			return
		}
		ctx, cancel := context.WithDeadline(r.Context(), expires)
		defer cancel()
		requestKey, _ := idKey(req.ID)
		requestKey = g.ID + ":" + requestKey
		activeMu.Lock()
		if _, ok := active[requestKey]; ok {
			activeMu.Unlock()
			w.Header().Set("Content-Type", "application/json")
			w.WriteHeader(http.StatusConflict)
			_ = json.NewEncoder(w).Encode(failure(req.ID, -32600, "duplicate in-flight request id"))
			return
		}
		active[requestKey] = cancel
		activeMu.Unlock()
		defer func() { activeMu.Lock(); delete(active, requestKey); activeMu.Unlock() }()
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(handle(ctx, s, req, &g, true))
	})
}

// RunHTTP requires TLS for non-loopback listeners. Preconfigured bearer grants
// are supported; this is not an OAuth discovery service or a remote-worker API.
func RunHTTP(ctx context.Context, s *service.Service, address, cert, key string, ready func(string) error) error {
	host, _, e := net.SplitHostPort(address)
	if e != nil {
		return e
	}
	ip := net.ParseIP(host)
	local := ip != nil && ip.IsLoopback()
	if !local && (cert == "" || key == "") {
		return errors.New("non-loopback binding requires TLS certificate and key")
	}
	if (cert == "") != (key == "") {
		return errors.New("both TLS files required")
	}
	ln, e := net.Listen("tcp", address)
	if e != nil {
		return e
	}
	defer ln.Close()
	server := &http.Server{Handler: HTTP(s), ReadHeaderTimeout: 5 * time.Second, ReadTimeout: 30 * time.Second, IdleTimeout: 60 * time.Second, MaxHeaderBytes: 16 << 10, TLSConfig: &tls.Config{MinVersion: tls.VersionTLS13}}
	if cert != "" {
		pair, e := tls.LoadX509KeyPair(cert, key)
		if e != nil {
			return e
		}
		server.TLSConfig.Certificates = []tls.Certificate{pair}
		ln = tls.NewListener(ln, server.TLSConfig)
	}
	if ready != nil {
		if e = ready(ln.Addr().String()); e != nil {
			return e
		}
	}
	done := make(chan error, 1)
	go func() { done <- server.Serve(ln) }()
	select {
	case e = <-done:
		if errors.Is(e, http.ErrServerClosed) {
			return nil
		}
		return e
	case <-ctx.Done():
		shutdown, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		e = server.Shutdown(shutdown)
		if e != nil {
			server.Close()
		}
		<-done
		return e
	}
}
