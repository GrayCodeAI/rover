package store

import (
	"encoding/json"
	"errors"
	"fmt"
	"github.com/GrayCodeAI/rover/internal/model"
)

var ErrBusy = errors.New("resources busy")

type Lease struct {
	Owner      string `json:"owner"`
	Resource   string `json:"resource"`
	AcquiredAt string `json:"acquired_at"`
}

// Acquire is an atomic coordination lock, not filesystem access control. There
// is no unsafe lease timeout/takeover while an old process may still be running.
func (s *Store) Acquire(owner string, resources []string, capacity int) error {
	if !model.ValidID(owner) || capacity < 1 || capacity > 128 {
		return errors.New("invalid resource request")
	}
	for _, k := range resources {
		if !model.ValidID(k) || len(k) > 96 {
			return errors.New("invalid resource name")
		}
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.transaction(func() error {
		rows, e := s.query("SELECT id,payload FROM records WHERE kind='lease'")
		if e != nil {
			return e
		}
		owners := map[string]bool{}
		occupied := map[string]string{}
		for _, row := range rows {
			var l Lease
			if e = json.Unmarshal([]byte(row[1]), &l); e != nil {
				return e
			}
			owners[l.Owner] = true
			occupied[row[0]] = l.Owner
		}
		if !owners[owner] && len(owners) >= capacity {
			return ErrBusy
		}
		keys := []string{"slot." + owner}
		for _, k := range resources {
			keys = append(keys, "resource."+k)
		}
		for _, k := range keys {
			if o := occupied[k]; o != "" && o != owner {
				return ErrBusy
			}
		}
		for _, k := range keys {
			if e = s.put("lease", k, Lease{owner, k, model.Now()}, ""); e != nil {
				return e
			}
		}
		return nil
	})
}
func (s *Store) Release(owner string) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.transaction(func() error {
		rows, e := s.query("SELECT id,payload FROM records WHERE kind='lease'")
		if e != nil {
			return e
		}
		for _, row := range rows {
			var l Lease
			if e = json.Unmarshal([]byte(row[1]), &l); e != nil {
				return e
			}
			if l.Owner == owner {
				if _, e = s.query("DELETE FROM records WHERE kind='lease' AND id=?", row[0]); e != nil {
					return e
				}
			}
		}
		return nil
	})
}
func (s *Store) Capacity() (int, error) {
	var v struct {
		MaxAgents int `json:"max_agents"`
	}
	e := s.Get("settings", "runtime", &v)
	if errors.Is(e, ErrNotFound) {
		return 4, nil
	}
	if e != nil {
		return 0, e
	}
	if v.MaxAgents < 1 || v.MaxAgents > 128 {
		return 0, errors.New("invalid max_agents")
	}
	return v.MaxAgents, nil
}
func (s *Store) Delete(kind, id, event string) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.transaction(func() error {
		r, e := s.query("SELECT id FROM records WHERE kind=? AND id=?", kind, id)
		if e != nil {
			return e
		}
		if len(r) == 0 {
			return ErrNotFound
		}
		if _, e = s.query("DELETE FROM records WHERE kind=? AND id=?", kind, id); e != nil {
			return e
		}
		if event != "" {
			_, e = s.query("INSERT INTO events(kind,ref,payload,at) VALUES(?,?,?,?)", event, id, `{"deleted":true}`, model.Now())
		}
		return e
	})
}
func (s *Store) ListAll(kind string, max int) ([]json.RawMessage, error) {
	if max < 1 || max > 100000 {
		return nil, errors.New("invalid bound")
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	rows, e := s.query("SELECT payload FROM records WHERE kind=? ORDER BY updated DESC,id DESC LIMIT ?", kind, fmt.Sprint(max+1))
	if e != nil {
		return nil, e
	}
	if len(rows) > max {
		return nil, errors.New("collection exceeds explicit bound; refusing partial answer")
	}
	out := make([]json.RawMessage, 0, len(rows))
	for _, r := range rows {
		out = append(out, json.RawMessage(r[0]))
	}
	return out, nil
}
