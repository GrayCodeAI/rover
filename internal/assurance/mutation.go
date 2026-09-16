package assurance

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"github.com/GrayCodeAI/rover/internal/model"
	"github.com/GrayCodeAI/rover/internal/source"
	"github.com/GrayCodeAI/rover/internal/store"
)

type Mutation struct {
	ID     string `json:"id"`
	Path   string `json:"path"`
	Before string `json:"before"`
	After  string `json:"after"`
}
type MutationSpec struct {
	Schema    string     `json:"schema"`
	CheckIDs  []string   `json:"check_ids"`
	Mutations []Mutation `json:"mutations"`
}
type MutationResult struct {
	Mutation      Mutation `json:"mutation"`
	Snapshot      string   `json:"snapshot"`
	Investigation string   `json:"investigation"`
	Result        string   `json:"result"`
}
type MutationReport struct {
	Schema                string           `json:"schema"`
	ID                    string           `json:"id"`
	Candidate             string           `json:"candidate"`
	BaselineInvestigation string           `json:"baseline_investigation"`
	Results               []MutationResult `json:"results"`
	Limits                string           `json:"limits"`
}

func Mutate(ctx context.Context, s *store.Store, base, cand model.Snapshot, cfg model.Config, sp MutationSpec, o Options) (r MutationReport, e error) {
	r = MutationReport{Schema: model.Schema, ID: model.ID("mutation"), Candidate: cand.ID, Results: []MutationResult{}, Limits: "explicit synthetic mutations only; survivors may be equivalent; no universal test adequacy score"}
	if sp.Schema != model.Schema || len(sp.Mutations) == 0 || len(sp.Mutations) > 64 || len(sp.CheckIDs) == 0 {
		return r, errors.New("mutation spec requires 1..64 explicit mutants and approved check IDs")
	}
	ids := map[string]bool{}
	c := cfg
	c.Checks = nil
	for _, id := range sp.CheckIDs {
		if ids[id] {
			return r, errors.New("duplicate check")
		}
		ids[id] = true
		found := false
		for _, x := range cfg.Checks {
			if x.ID == id {
				x.Required = true
				c.Checks = append(c.Checks, x)
				found = true
			}
		}
		if !found {
			return r, fmt.Errorf("unapproved check: %s", id)
		}
	}
	mIDs := map[string]bool{}
	for _, m := range sp.Mutations {
		if !model.ValidID(m.ID) || mIDs[m.ID] || m.Before == "" || m.Before == m.After {
			return r, errors.New("invalid or duplicate mutation")
		}
		mIDs[m.ID] = true
		b, e := source.FileContent(s, cand, m.Path)
		if e != nil {
			return r, e
		}
		if bytes.Count(b, []byte(m.Before)) != 1 {
			return r, fmt.Errorf("mutation %s must match exactly once", m.ID)
		}
	}
	bi, e := Verify(ctx, s, base, cand, c, o)
	if e != nil {
		return r, e
	}
	r.BaselineInvestigation = bi.ID
	if len(bi.Checks) != len(c.Checks) {
		return r, errors.New("baseline incomplete")
	}
	for _, x := range bi.Checks {
		if x.Outcome != "PASS" {
			return r, errors.New("unmodified baseline must pass every selected check")
		}
	}
	for _, m := range sp.Mutations {
		if e = ctx.Err(); e != nil {
			return r, e
		}
		b, e := source.FileContent(s, cand, m.Path)
		if e != nil {
			return r, e
		}
		next := bytes.Replace(b, []byte(m.Before), []byte(m.After), 1)
		h, e := s.Blob(next)
		if e != nil {
			return r, e
		}
		fs := append([]model.File(nil), cand.Files...)
		for i := range fs {
			if fs[i].Path == m.Path {
				fs[i].SHA256 = h
				fs[i].Size = int64(len(next))
			}
		}
		snap, e := source.Compose(s, cand, fs, "mutation:"+m.ID)
		if e != nil {
			return r, e
		}
		in, e := Verify(ctx, s, base, snap, c, o)
		if e != nil {
			return r, e
		}
		result := "SURVIVED"
		for _, x := range in.Checks {
			if x.Outcome == "FAIL" {
				result = "KILLED"
			}
		}
		for _, x := range in.Checks {
			if x.Outcome == "ERROR" || x.Outcome == "INCONCLUSIVE" {
				result = "INCONCLUSIVE"
			}
		}
		if len(in.Checks) != len(c.Checks) {
			result = "INCONCLUSIVE"
		}
		r.Results = append(r.Results, MutationResult{m, snap.ID, in.ID, result})
		if e = s.Put("mutation", r.ID, r, "mutation.progress"); e != nil {
			return r, e
		}
	}
	return r, s.Put("mutation", r.ID, r, "mutation.completed")
}
