// Package learning evaluates bounded check-order proposals. It does not train a
// coding model, relax mandatory checks, declare software correct, or promote its
// own changes. Historical failures are observations, not automatically defects.
package learning

import (
	"encoding/json"
	"errors"
	"fmt"
	"math"
	"sort"
	"strings"
	"time"

	"github.com/GrayCodeAI/rover/internal/config"
	"github.com/GrayCodeAI/rover/internal/model"
	"github.com/GrayCodeAI/rover/internal/store"
)

type Observation struct {
	Check       string  `json:"check"`
	Samples     int     `json:"samples"`
	Failures    int     `json:"failures"`
	Errors      int     `json:"errors"`
	MeanSeconds float64 `json:"mean_seconds"`
	Priority    float64 `json:"priority"`
}
type Proposal struct {
	Schema             string        `json:"schema"`
	ID                 string        `json:"id"`
	Repository         string        `json:"repository"`
	ConfigDigest       string        `json:"config_digest"`
	Order              []string      `json:"order"`
	OriginalOrder      []string      `json:"original_order"`
	Statistics         []Observation `json:"statistics"`
	TrainingCandidates []string      `json:"training_candidates"`
	CreatedAt          string        `json:"created_at"`
	Method             string        `json:"method"`
	Status             string        `json:"status"`
}
type Case struct {
	Investigation string `json:"investigation"`
	Attribution   string `json:"attribution"`
	Label         string `json:"label"`
	Synthetic     bool   `json:"synthetic"`
}
type Dataset struct {
	Schema      string `json:"schema"`
	ID          string `json:"id"`
	Partition   string `json:"partition"`
	Cases       []Case `json:"cases"`
	Description string `json:"description"`
}
type Evaluation struct {
	Schema                  string  `json:"schema"`
	ID                      string  `json:"id"`
	Proposal                string  `json:"proposal"`
	ProposalDigest          string  `json:"proposal_digest"`
	DatasetDigest           string  `json:"dataset_digest"`
	Dataset                 string  `json:"dataset"`
	ConfigDigest            string  `json:"config_digest"`
	Cases                   int     `json:"cases"`
	FailureCases            int     `json:"failure_cases"`
	Inconclusive            int     `json:"inconclusive"`
	BaselineSecondsToSignal float64 `json:"baseline_seconds_to_signal"`
	ProposedSecondsToSignal float64 `json:"proposed_seconds_to_signal"`
	ConfirmedDefectDetected int     `json:"confirmed_defect_detected"`
	ConfirmedCleanBlocked   int     `json:"confirmed_clean_blocked"`
	Eligible                bool    `json:"eligible"`
	Reason                  string  `json:"reason"`
	RepeatedHoldoutUse      int     `json:"repeated_holdout_use"`
	CreatedAt               string  `json:"created_at"`
}
type Promotion struct {
	Schema       string   `json:"schema"`
	ID           string   `json:"id"`
	Proposal     string   `json:"proposal"`
	Evaluation   string   `json:"evaluation"`
	ConfigDigest string   `json:"config_digest"`
	Order        []string `json:"order"`
	Note         string   `json:"note"`
	Authority    string   `json:"authority"`
	At           string   `json:"at"`
	Revoked      bool     `json:"revoked"`
}

func ValidateOrder(c model.Config, order []string) error {
	if len(order) != len(c.Checks) {
		return errors.New("strategy must include every configured check exactly once")
	}
	known := map[string]bool{}
	for _, ch := range c.Checks {
		known[ch.ID] = true
	}
	seen := map[string]bool{}
	for _, id := range order {
		if !known[id] || seen[id] {
			return errors.New("strategy has unknown or duplicate check")
		}
		seen[id] = true
	}
	return nil
}
func Suggest(s *store.Store, repository string, c model.Config) (Proposal, error) {
	p := Proposal{Schema: model.Schema, ID: model.ID("proposal"), Repository: repository, ConfigDigest: model.Hash(c), CreatedAt: model.Now(), Method: "bounded UCB-style failure-signal/cost ranking; observed check failures are not confirmed defects; recommendation only", Status: "PROPOSED"}
	if e := config.Validate(c); e != nil {
		return p, e
	}
	stats := map[string]*Observation{}
	specs := map[string]string{}
	for _, ch := range c.Checks {
		stats[ch.ID] = &Observation{Check: ch.ID}
		specs[ch.ID] = model.Hash(ch)
		p.OriginalOrder = append(p.OriginalOrder, ch.ID)
	}
	rows, e := s.ListAll("investigation", 100000)
	if e != nil {
		return p, e
	}
	train := map[string]bool{}
	total := 0
	for _, raw := range rows {
		var in model.Investigation
		if e = json.Unmarshal(raw, &in); e != nil {
			return p, e
		}
		if in.Repository != repository || in.ConfigDigest != p.ConfigDigest || in.FinishedAt == "" {
			continue
		}
		used := false
		for _, r := range in.Checks {
			v := stats[r.ID]
			if v == nil || r.SpecDigest != specs[r.ID] || r.Candidate != in.Candidate {
				continue
			}
			if r.Outcome != "PASS" && r.Outcome != "FAIL" {
				v.Errors++
				continue
			}
			duration, ok := seconds(r)
			if !ok {
				v.Errors++
				continue
			}
			v.Samples++
			if r.Outcome == "FAIL" {
				v.Failures++
			}
			v.MeanSeconds += duration
			total++
			used = true
		}
		if used {
			train[in.Candidate] = true
		}
	}
	for _, v := range stats {
		if v.Samples > 0 {
			v.MeanSeconds /= float64(v.Samples)
			reward := float64(v.Failures) / float64(v.Samples)
			v.Priority = (reward + math.Sqrt(2*math.Log(float64(total)+1)/float64(v.Samples))) / math.Max(v.MeanSeconds, 0.001)
		}
		p.Statistics = append(p.Statistics, *v)
	}
	sort.Slice(p.Statistics, func(i, j int) bool {
		a, b := p.Statistics[i], p.Statistics[j]
		if (a.Samples == 0) != (b.Samples == 0) {
			return a.Samples == 0
		}
		if a.Priority == b.Priority {
			return a.Check < b.Check
		}
		return a.Priority > b.Priority
	})
	for _, v := range p.Statistics {
		p.Order = append(p.Order, v.Check)
	}
	for k := range train {
		p.TrainingCandidates = append(p.TrainingCandidates, k)
	}
	sort.Strings(p.TrainingCandidates)
	if e = ValidateOrder(c, p.Order); e != nil {
		return p, e
	}
	return p, s.Put("proposal", p.ID, p, "learning.proposed")
}
func Import(s *store.Store, d Dataset) (Dataset, error) {
	if d.Schema != model.Schema || (d.Partition != "holdout" && d.Partition != "training") || len(d.Cases) == 0 || len(d.Cases) > 10000 || strings.TrimSpace(d.Description) == "" {
		return d, errors.New("dataset needs schema, partition, description and 1..10000 cases")
	}
	seen := map[string]bool{}
	repo := ""
	for _, c := range d.Cases {
		if !model.ValidID(c.Investigation) || seen[c.Investigation] || strings.TrimSpace(c.Attribution) == "" {
			return d, errors.New("case requires unique investigation and explicit attribution")
		}
		seen[c.Investigation] = true
		switch c.Label {
		case "confirmed-defect", "confirmed-clean", "unknown":
		default:
			return d, errors.New("unsupported outcome label")
		}
		var in model.Investigation
		if e := s.Get("investigation", c.Investigation, &in); e != nil {
			return d, e
		}
		if in.FinishedAt == "" {
			return d, errors.New("dataset contains unfinished investigation")
		}
		if repo == "" {
			repo = in.Repository
		} else if in.Repository != repo {
			return d, errors.New("dataset mixes repositories; import one repository per dataset")
		}
	}
	d.ID = "dataset_"
	d.ID = "dataset_" + model.Hash(d)
	return d, s.Put("dataset", d.ID, d, "dataset.imported")
}
func seconds(r model.CheckResult) (float64, bool) {
	a, e := time.Parse(time.RFC3339Nano, r.Process.StartedAt)
	if e != nil {
		return 0, false
	}
	b, e := time.Parse(time.RFC3339Nano, r.Process.FinishedAt)
	if e != nil || b.Before(a) {
		return 0, false
	}
	return b.Sub(a).Seconds(), true
}
func signal(in model.Investigation, c model.Config, order []string) (float64, bool, bool, error) {
	by := map[string]model.CheckResult{}
	for _, r := range in.Checks {
		if _, ok := by[r.ID]; ok {
			return 0, false, false, errors.New("duplicate check evidence")
		}
		by[r.ID] = r
	}
	valid := true
	for _, ch := range c.Checks {
		r, ok := by[ch.ID]
		if !ok || r.Candidate != in.Candidate || r.SpecDigest != model.Hash(ch) || (r.Outcome != "PASS" && r.Outcome != "FAIL") {
			valid = false
		}
	}
	elapsed := 0.0
	failure := false
	for _, id := range order {
		r := by[id]
		dt, ok := seconds(r)
		if !ok {
			valid = false
		}
		elapsed += dt
		if r.Outcome == "FAIL" {
			failure = true
			break
		}
	}
	return elapsed, failure, valid, nil
}
func Evaluate(s *store.Store, proposalID, datasetID string) (Evaluation, error) {
	e := Evaluation{Schema: model.Schema, ID: model.ID("eval"), Proposal: proposalID, Dataset: datasetID, CreatedAt: model.Now()}
	var p Proposal
	if err := s.Get("proposal", proposalID, &p); err != nil {
		return e, err
	}
	var d Dataset
	if err := s.Get("dataset", datasetID, &d); err != nil {
		return e, err
	}
	if d.Partition != "holdout" {
		return e, errors.New("evaluation requires an explicit holdout dataset")
	}
	b, err := s.ReadBlob(p.ConfigDigest)
	if err != nil {
		return e, err
	}
	var cfg model.Config
	if err = config.Decode(b, &cfg); err != nil {
		return e, err
	}
	if err = ValidateOrder(cfg, p.Order); err != nil {
		return e, err
	}
	if err = ValidateOrder(cfg, p.OriginalOrder); err != nil {
		return e, err
	}
	train := map[string]bool{}
	for _, id := range p.TrainingCandidates {
		train[id] = true
	}
	e.ConfigDigest = p.ConfigDigest
	e.ProposalDigest = model.Hash(p)
	e.DatasetDigest = model.Hash(d)
	seen := map[string]bool{}
	for _, c := range d.Cases {
		var in model.Investigation
		if err = s.Get("investigation", c.Investigation, &in); err != nil {
			return e, err
		}
		if in.Repository != p.Repository || in.ConfigDigest != p.ConfigDigest {
			return e, errors.New("holdout evidence project/config mismatch")
		}
		if train[in.Candidate] {
			return e, errors.New("holdout candidate overlaps proposal training data")
		}
		if seen[in.Candidate] {
			return e, errors.New("duplicate candidate in holdout")
		}
		seen[in.Candidate] = true
		old, failed, valid, err := signal(in, cfg, p.OriginalOrder)
		if err != nil {
			return e, err
		}
		newer, newFailed, newValid, err := signal(in, cfg, p.Order)
		if err != nil {
			return e, err
		}
		e.Cases++
		if !valid || !newValid || newFailed != failed {
			e.Inconclusive++
			continue
		}
		e.BaselineSecondsToSignal += old
		e.ProposedSecondsToSignal += newer
		if failed {
			e.FailureCases++
			if c.Label == "confirmed-defect" {
				e.ConfirmedDefectDetected++
			}
			if c.Label == "confirmed-clean" {
				e.ConfirmedCleanBlocked++
			}
		}
	}
	past, err := s.ListAll("evaluation", 100000)
	if err != nil {
		return e, err
	}
	for _, b := range past {
		var x Evaluation
		if json.Unmarshal(b, &x) == nil && x.Dataset == datasetID {
			e.RepeatedHoldoutUse++
		}
	}
	e.Eligible = e.Cases > 0 && e.FailureCases > 0 && e.Inconclusive == 0 && e.ProposedSecondsToSignal < e.BaselineSecondsToSignal
	if e.RepeatedHoldoutUse >= 3 {
		e.Eligible = false
	}
	e.Reason = "offline order replay; all checks remain mandatory/optional as configured; timing assumes independent checks and does not establish live benefit or causal defect detection"
	return e, s.Put("evaluation", e.ID, e, "learning.evaluated")
}
func Promote(s *store.Store, evaluationID, note string) (Promotion, error) {
	r := Promotion{}
	if strings.TrimSpace(note) == "" {
		return r, errors.New("explicit operator review note required")
	}
	var e Evaluation
	if err := s.Get("evaluation", evaluationID, &e); err != nil {
		return r, err
	}
	if !e.Eligible {
		return r, errors.New("evaluation does not satisfy promotion criteria")
	}
	var p Proposal
	if err := s.Get("proposal", e.Proposal, &p); err != nil {
		return r, err
	}
	if p.ConfigDigest != e.ConfigDigest || model.Hash(p) != e.ProposalDigest {
		return r, errors.New("proposal changed after evaluation")
	}
	var dataset Dataset
	if err := s.Get("dataset", e.Dataset, &dataset); err != nil {
		return r, err
	}
	if model.Hash(dataset) != e.DatasetDigest {
		return r, errors.New("dataset changed after evaluation")
	}
	r = Promotion{Schema: model.Schema, ID: model.ID("strategy"), Proposal: p.ID, Evaluation: e.ID, ConfigDigest: p.ConfigDigest, Order: append([]string(nil), p.Order...), Note: note, Authority: "explicit same-user local operator; not an independent organization promotion authority", At: model.Now()}
	return r, s.Put("strategy", r.ID, r, "learning.promoted")
}
func LoadOrder(s *store.Store, id string, c model.Config) ([]string, error) {
	var p Promotion
	if e := s.Get("strategy", id, &p); e != nil {
		return nil, e
	}
	if p.Revoked || p.ConfigDigest != model.Hash(c) {
		return nil, errors.New("strategy revoked or approved configuration changed")
	}
	if e := ValidateOrder(c, p.Order); e != nil {
		return nil, e
	}
	return append([]string(nil), p.Order...), nil
}
func Explain(s *store.Store, id string) (any, error) {
	for _, kind := range []string{"proposal", "dataset", "evaluation", "strategy"} {
		var v json.RawMessage
		if e := s.Get(kind, id, &v); e == nil {
			return v, nil
		} else if !errors.Is(e, store.ErrNotFound) {
			return nil, e
		}
	}
	return nil, fmt.Errorf("learning record not found")
}
