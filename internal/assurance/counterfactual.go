package assurance

import (
	"bufio"
	"bytes"
	"context"
	"encoding/json"
	"encoding/xml"
	"errors"
	"fmt"
	"github.com/GrayCodeAI/rover/internal/model"
	"github.com/GrayCodeAI/rover/internal/source"
	"github.com/GrayCodeAI/rover/internal/store"
	"strings"
)

type RegressionSpec struct {
	Schema          string   `json:"schema"`
	CheckID         string   `json:"check_id"`
	TestPaths       []string `json:"test_paths"`
	TestID          string   `json:"test_id"`
	FailureContains string   `json:"failure_contains"`
}
type RegressionResult struct {
	Schema                 string         `json:"schema"`
	ID                     string         `json:"id"`
	Base                   string         `json:"base"`
	Candidate              string         `json:"candidate"`
	Overlay                string         `json:"overlay"`
	Spec                   RegressionSpec `json:"spec"`
	BaseInvestigation      string         `json:"base_investigation"`
	CandidateInvestigation string         `json:"candidate_investigation"`
	Assessment             string         `json:"assessment"`
	Reason                 string         `json:"reason"`
}

func ProveRegression(ctx context.Context, s *store.Store, base, cand model.Snapshot, cfg model.Config, sp RegressionSpec, o Options) (r RegressionResult, e error) {
	r = RegressionResult{Schema: model.Schema, ID: model.ID("regression"), Base: base.ID, Candidate: cand.ID, Spec: sp, Assessment: "INCONCLUSIVE"}
	if sp.Schema != model.Schema || sp.CheckID == "" || sp.TestID == "" || len(sp.TestID) > 4096 || strings.TrimSpace(sp.FailureContains) == "" || len(sp.FailureContains) > 4096 {
		return r, errors.New("explicit check, unique test identity and expected failure signature required")
	}
	if len(sp.TestPaths) < 1 || len(sp.TestPaths) > 32 {
		return r, errors.New("explicit test paths required (max 32)")
	}
	for _, p := range sp.TestPaths {
		if source.Category(p) != "test" {
			return r, fmt.Errorf("overlay must name test-classified files: %s", p)
		}
	}
	var check model.CheckSpec
	for _, c := range cfg.Checks {
		if c.ID == sp.CheckID {
			check = c
		}
	}
	if check.ID == "" || (check.Parser != "go-test-json" && check.Parser != "junit") {
		return r, errors.New("counterfactual requires an approved Go JSON or JUnit check")
	}
	if base.Repository != cand.Repository {
		return r, errors.New("repository mismatch")
	}
	c := cfg
	c.Checks = []model.CheckSpec{check}
	c.Checks[0].Required = true
	overlay, e := source.Overlay(s, base, cand, sp.TestPaths)
	if e != nil {
		return r, e
	}
	r.Overlay = overlay.ID
	bi, e := Verify(ctx, s, base, overlay, c, o)
	if e != nil {
		return r, e
	}
	r.BaseInvestigation = bi.ID
	ci, e := Verify(ctx, s, base, cand, c, o)
	if e != nil {
		return r, e
	}
	r.CandidateInvestigation = ci.ID
	if len(bi.Checks) != 1 || len(ci.Checks) != 1 {
		return r, errors.New("expected exactly one executed check")
	}
	b, n := bi.Checks[0], ci.Checks[0]
	switch {
	case n.Outcome != "PASS":
		r.Reason = "candidate did not pass the approved check"
	case b.Outcome == "PASS":
		r.Assessment = "UNSUPPORTED"
		r.Reason = "regression test also passes on the base"
	case b.Outcome != "FAIL":
		r.Reason = "base execution was an error/inconclusive; setup failures are not a reproduction"
	default:
		old, e := checkReport(s, b)
		if e != nil {
			return r, e
		}
		new, e := checkReport(s, n)
		if e != nil {
			return r, e
		}
		failure, uniqueOld := testEvidence(check.Parser, old, sp.TestID)
		pass, uniqueNew := testEvidence(check.Parser, new, sp.TestID)
		if uniqueOld && uniqueNew && failure.Status == "FAIL" && strings.Contains(failure.Failure, sp.FailureContains) && pass.Status == "PASS" && failure.Identity == pass.Identity {
			r.Assessment = "SUPPORTED_WITHIN_SCOPE"
			r.Reason = "the same uniquely identified test reproduced the specified failure on base and passed on candidate; not universal correctness"
		} else {
			r.Reason = "expected failure/pass identity could not be established uniquely; build/import/setup errors are insufficient"
		}
	}
	return r, s.Put("regression", r.ID, r, "regression.completed")
}
func checkReport(s *store.Store, c model.CheckResult) ([]byte, error) {
	if c.Parser == "junit" {
		return s.ReadBlob(c.ReportSHA256)
	}
	return s.ReadBlob(c.Process.StdoutSHA256)
}

type testObservation struct{ Identity, Status, Failure string }

func testEvidence(parser string, b []byte, id string) (testObservation, bool) {
	observations := map[string]testObservation{}
	duplicates := map[string]bool{}
	if parser == "go-test-json" {
		sc := bufio.NewScanner(bytes.NewReader(b))
		sc.Buffer(make([]byte, 4096), 1<<20)
		output := map[string]string{}
		for sc.Scan() {
			var e struct{ Action, Package, Test, Output string }
			if json.Unmarshal(sc.Bytes(), &e) != nil {
				return testObservation{}, false
			}
			if e.Test == "" {
				continue
			}
			key := e.Package + "::" + e.Test
			if id != key && id != e.Test {
				continue
			}
			if e.Action == "output" {
				output[key] += e.Output
			}
			if e.Action == "pass" || e.Action == "fail" || e.Action == "skip" {
				if _, ok := observations[key]; ok {
					duplicates[key] = true
				}
				observations[key] = testObservation{key, strings.ToUpper(e.Action), output[key]}
			}
		}
		if sc.Err() != nil {
			return testObservation{}, false
		}
	} else {
		var root regressionSuite
		if e := xml.Unmarshal(b, &root); e != nil {
			return testObservation{}, false
		}
		var walk func(regressionSuite)
		walk = func(s regressionSuite) {
			for _, c := range s.Cases {
				key := c.ClassName + "::" + c.Name
				if id != key && id != c.Name {
					continue
				}
				o := testObservation{Identity: key, Status: "PASS"}
				if c.Error != nil {
					o.Status = "ERROR"
				}
				if c.Failure != nil {
					o.Status = "FAIL"
					o.Failure = c.Failure.Message + "\n" + c.Failure.Text
				}
				if c.Skipped != nil {
					o.Status = "SKIP"
				}
				if _, ok := observations[key]; ok {
					duplicates[key] = true
				}
				observations[key] = o
			}
			for _, n := range s.Suites {
				walk(n)
			}
		}
		walk(root)
	}
	if len(observations) != 1 {
		return testObservation{}, false
	}
	for k, v := range observations {
		return v, !duplicates[k]
	}
	return testObservation{}, false
}

// Replay re-executes retained source bytes under the original approved config.
// It produces a new investigation; historical observations are never overwritten.
func Replay(ctx context.Context, s *store.Store, id string, o Options) (model.Investigation, error) {
	var old model.Investigation
	if e := s.Get("investigation", id, &old); e != nil {
		return model.Investigation{}, e
	}
	base, e := source.Load(s, old.Base)
	if e != nil {
		return model.Investigation{}, e
	}
	cand, e := source.Load(s, old.Candidate)
	if e != nil {
		return model.Investigation{}, e
	}
	cb, e := s.ReadBlob(old.ConfigDigest)
	if e != nil {
		return model.Investigation{}, e
	}
	var cfg model.Config
	if e = json.Unmarshal(cb, &cfg); e != nil {
		return model.Investigation{}, e
	}
	if model.Hash(cfg) != old.ConfigDigest {
		return model.Investigation{}, errors.New("retained config identity mismatch")
	}
	o.PolicySource = "replay:" + old.PolicySource
	return Verify(ctx, s, base, cand, cfg, o)
}

type regressionFailure struct {
	Message string `xml:"message,attr"`
	Text    string `xml:",chardata"`
}
type regressionCase struct {
	Name      string             `xml:"name,attr"`
	ClassName string             `xml:"classname,attr"`
	Failure   *regressionFailure `xml:"failure"`
	Error     *regressionFailure `xml:"error"`
	Skipped   *struct{}          `xml:"skipped"`
}
type regressionSuite struct {
	Cases  []regressionCase  `xml:"testcase"`
	Suites []regressionSuite `xml:"testsuite"`
}
