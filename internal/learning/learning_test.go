package learning

import (
	"encoding/json"
	"strings"
	"testing"
	"time"

	"github.com/GrayCodeAI/rover/internal/model"
	"github.com/GrayCodeAI/rover/internal/store"
	"github.com/GrayCodeAI/rover/internal/testutil"
)

func fixture(t *testing.T, s *store.Store, repo string, c model.Config, candidate string) model.Investigation {
	t.Helper()
	in := model.Investigation{Schema: model.Schema, ID: model.ID("inv"), Repository: repo, Candidate: candidate, ConfigDigest: model.Hash(c), FinishedAt: model.Now(), Decision: "BLOCKED"}
	at := time.Date(2025, 1, 1, 0, 0, 0, 0, time.UTC)
	for i, ch := range c.Checks {
		out := "PASS"
		dt := 20 * time.Second
		if i == 1 {
			out = "FAIL"
			dt = time.Second
		}
		in.Checks = append(in.Checks, model.CheckResult{ID: ch.ID, Candidate: candidate, SpecDigest: model.Hash(ch), Parser: ch.Parser, Outcome: out, Process: model.ProcessResult{StartedAt: at.Format(time.RFC3339Nano), FinishedAt: at.Add(dt).Format(time.RFC3339Nano)}})
	}
	if e := s.Put("investigation", in.ID, in, "fixture.synthetic"); e != nil {
		t.Fatal(e)
	}
	b, _ := json.Marshal(c)
	s.Blob(b)
	return in
}
func setup(t *testing.T) (string, *store.Store, model.Config) {
	r, s := testutil.Repo(t, map[string]string{"a": "b"})
	c := model.Config{Schema: model.Schema, Checks: []model.CheckSpec{{ID: "slow", Argv: []string{"true"}, Parser: "exit-code", Required: true, Timeout: "1m"}, {ID: "fast", Argv: []string{"true"}, Parser: "exit-code", Required: true, Timeout: "1m"}}}
	return r, s, c
}
func TestProposeHoldoutPromoteAndRevoke(t *testing.T) {
	repo, s, c := setup(t)
	fixture(t, s, repo, c, model.ID("snap"))
	p, e := Suggest(s, repo, c)
	if e != nil {
		t.Fatal(e)
	}
	if p.Order[0] != "fast" {
		t.Fatal(p.Order)
	}
	held := fixture(t, s, repo, c, model.ID("snap"))
	d, e := Import(s, Dataset{Schema: model.Schema, Partition: "holdout", Description: "synthetic timing fixture, not live benchmark", Cases: []Case{{Investigation: held.ID, Attribution: "unit-test oracle", Label: "confirmed-defect", Synthetic: true}}})
	if e != nil {
		t.Fatal(e)
	}
	ev, e := Evaluate(s, p.ID, d.ID)
	if e != nil {
		t.Fatal(e)
	}
	if !ev.Eligible || ev.BaselineSecondsToSignal != 21 || ev.ProposedSecondsToSignal != 1 {
		t.Fatal(ev)
	}
	second, e := Evaluate(s, p.ID, d.ID)
	if e != nil || second.RepeatedHoldoutUse != 1 {
		t.Fatal(second, e)
	}
	promoted, e := Promote(s, ev.ID, "explicit local review")
	if e != nil {
		t.Fatal(e)
	}
	order, e := LoadOrder(s, promoted.ID, c)
	if e != nil || order[0] != "fast" {
		t.Fatal(order, e)
	}
	changed := c
	changed.Checks = append([]model.CheckSpec(nil), c.Checks...)
	changed.Checks[0].Timeout = "2m"
	if _, e = LoadOrder(s, promoted.ID, changed); e == nil {
		t.Fatal("strategy accepted changed configuration")
	}
	promoted.Revoked = true
	s.Put("strategy", promoted.ID, promoted, "fixture.revoke")
	if _, e = LoadOrder(s, promoted.ID, c); e == nil {
		t.Fatal("revoked strategy used")
	}
	// Mutating a proposal after evaluation invalidates promotion, even if it
	// remains a complete permutation of required checks.
	p.Order = []string{"slow", "fast"}
	s.Put("proposal", p.ID, p, "fixture.tamper")
	if _, e = Promote(s, ev.ID, "review"); e == nil {
		t.Fatal("accepted proposal changed after evaluation")
	}
}
func TestHoldoutLeakageAndMandatoryChecks(t *testing.T) {
	repo, s, c := setup(t)
	in := fixture(t, s, repo, c, model.ID("snap"))
	p, e := Suggest(s, repo, c)
	if e != nil {
		t.Fatal(e)
	}
	d, e := Import(s, Dataset{Schema: model.Schema, Partition: "holdout", Description: "deliberate overlap", Cases: []Case{{Investigation: in.ID, Attribution: "fixture", Label: "unknown"}}})
	if e != nil {
		t.Fatal(e)
	}
	if _, e = Evaluate(s, p.ID, d.ID); e == nil || !strings.Contains(e.Error(), "overlap") {
		t.Fatal(e)
	}
	for _, order := range [][]string{{"fast"}, {"fast", "fast"}, {"fast", "invented"}} {
		if e = ValidateOrder(c, order); e == nil {
			t.Fatal("accepted unsafe check order", order)
		}
	}
}

// TestNoAcceptanceCacheReuse covers A38: check outcomes are credited only when
// bound to the exact stored candidate. A result replayed against a different
// candidate is not reused (there is no acceptance cache to short-circuit replay).
func TestNoAcceptanceCacheReuse(t *testing.T) {
	repo, s, c := setup(t)
	one := fixture(t, s, repo, c, model.ID("snap_one"))
	two := fixture(t, s, repo, c, model.ID("snap_two"))
	candidate := model.ID("snap_three")
	at := time.Date(2025, 1, 1, 0, 0, 0, 0, time.UTC)
	mismatched := model.Investigation{Schema: model.Schema, ID: model.ID("inv"), Repository: repo, Candidate: candidate, ConfigDigest: model.Hash(c), FinishedAt: model.Now(), Decision: "BLOCKED"}
	for _, ch := range c.Checks {
		mismatched.Checks = append(mismatched.Checks, model.CheckResult{ID: ch.ID, Candidate: "snap_one", SpecDigest: model.Hash(ch), Parser: ch.Parser, Outcome: "PASS", Process: model.ProcessResult{StartedAt: at.Format(time.RFC3339Nano), FinishedAt: at.Add(time.Second).Format(time.RFC3339Nano)}})
	}
	if e := s.Put("investigation", mismatched.ID, mismatched, "fixture.mismatched-candidate"); e != nil {
		t.Fatal(e)
	}
	p, e := Suggest(s, repo, c)
	if e != nil {
		t.Fatal(e)
	}
	// two properly bound PASS results are counted; the mismatched-candidate
	// leftover (a stale cached-looking outcome) must not be credited.
	if p.Statistics[0].Samples != 2 {
		t.Fatalf("acceptance reused across candidates: %+v", p.Statistics)
	}
	// The original fixture candidates stay replayable from exact evidence.
	for _, in := range []model.Investigation{one, two} {
		var got model.Investigation
		if e = s.Get("investigation", in.ID, &got); e != nil {
			t.Fatal(e)
		}
	}
}
