package assurance

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"runtime"

	"github.com/GrayCodeAI/rover/internal/config"
	"github.com/GrayCodeAI/rover/internal/execution"
	"github.com/GrayCodeAI/rover/internal/learning"
	"github.com/GrayCodeAI/rover/internal/model"
	"github.com/GrayCodeAI/rover/internal/source"
	"github.com/GrayCodeAI/rover/internal/store"
)

type Options struct {
	Mode         string
	Image        string
	AllowLocal   bool
	PolicySource string
	Strategy     string
}

func LoadConfig(s *store.Store, base model.Snapshot, explicit string) (model.Config, string, error) {
	var c model.Config
	origin := "base-snapshot:" + base.ID
	if explicit != "" {
		if e := config.Read(explicit, &c); e != nil {
			return c, "", e
		}
		abs, e := filepath.Abs(explicit)
		if e != nil {
			return c, "", e
		}
		origin = "explicit-user-config:" + abs
	} else {
		b, e := source.FileContent(s, base, ".rover/config.json")
		if e != nil {
			return c, "", errors.New("approved base has no .rover/config.json; commit it first or explicitly authorize --config <path>")
		}
		if e = config.Decode(b, &c); e != nil {
			return c, "", e
		}
	}
	return c, origin, config.Validate(c)
}
func Decide(c model.Config, in *model.Investigation) {
	in.Decision = "INCONCLUSIVE"
	in.DecisionReason = "required verification is incomplete"
	required := 0
	bad := false
	unknown := false
	for _, spec := range c.Checks {
		if !spec.Required {
			continue
		}
		required++
		var got *model.CheckResult
		for i := range in.Checks {
			r := &in.Checks[i]
			if r.ID == spec.ID {
				if got != nil {
					unknown = true
				}
				got = r
			}
		}
		if got == nil {
			unknown = true
			continue
		}
		if got.SpecDigest != model.Hash(spec) || got.Candidate != in.Candidate {
			unknown = true
			continue
		}
		switch got.Outcome {
		case "FAIL":
			bad = true
		case "PASS":
		default:
			unknown = true
		}
	}
	if bad {
		in.Decision = "BLOCKED"
		in.DecisionReason = "at least one required check failed"
		return
	}
	if required == 0 {
		in.Unknowns = append(in.Unknowns, "No required checks are configured.")
		return
	}
	if unknown {
		return
	}
	if c.Policy.RequireReview || len(in.Findings) > 0 {
		in.Decision = "REVIEW_REQUIRED"
		in.DecisionReason = "required checks satisfied; explicit review policy or change finding applies"
		return
	}
	in.Decision = "ACCEPTED"
	in.DecisionReason = "configured required checks satisfied for this candidate under this policy; not proof of software correctness"
}
func Verify(ctx context.Context, s *store.Store, base, candidate model.Snapshot, c model.Config, o Options) (model.Investigation, error) {
	in := model.Investigation{}
	if e := config.Validate(c); e != nil {
		return in, e
	}
	if e := execution.Admit(o.Mode, o.AllowLocal, o.Image); e != nil {
		return in, e
	}
	cb, err := json.Marshal(c)
	if err != nil {
		return in, err
	}
	if _, err = s.Blob(cb); err != nil {
		return in, err
	}
	in = model.Investigation{Schema: model.Schema, ID: model.ID("inv"), RoverVersion: model.Version, StartedAt: model.Now(), Repository: candidate.Repository, Base: base.ID, Candidate: candidate.ID, ConfigDigest: model.Hash(c), PolicySource: o.PolicySource, Executor: o.Mode, Trust: "same-user local controller; not an independently protected merge gate", Environment: map[string]string{"os": runtime.GOOS, "arch": runtime.GOARCH, "go_compiler": runtime.Version(), "sqlite": store.SQLiteVersion()}, Checks: []model.CheckResult{}, Findings: []model.Finding{}, Unknowns: []string{"Passing configured checks does not establish all requirements, edge cases, security, or performance.", "No authoritative GitHub/CI publisher or authenticated team approval is implemented.", "External effects are not rolled back by process cancellation."}, Decision: "PENDING"}
	if o.Mode == "local-advisory" {
		in.Unknowns = append(in.Unknowns, "Local processes run with user permissions and can access host resources; no sandbox or complete action audit.")
	} else {
		in.Environment["container_image"] = o.Image
		in.Unknowns = append(in.Unknowns, "Docker adapter has not been live-tested in the build environment; isolation depends on the host and Docker.")
	}
	cmp := source.Compare(base, candidate)
	in.Findings = cmp.Findings
	integrity, e := TestIntegrity(s, base, candidate)
	if e != nil {
		return in, e
	}
	in.Findings = append(in.Findings, integrity...)
	for _, ch := range cmp.Changes {
		for _, pat := range c.Policy.ReviewPaths {
			re, _ := config.Glob(pat)
			if re.MatchString(ch.Path) {
				in.Findings = append(in.Findings, model.Finding{Rule: "review_path_changed", Message: "Approved policy requires review for this path", Path: ch.Path, Source: "configured_rule"})
			}
		}
	}
	if e := s.Put("investigation", in.ID, in, "investigation.started"); e != nil {
		return in, e
	}
	ordered := append([]model.CheckSpec(nil), c.Checks...)
	if o.Strategy != "" {
		order, e := learning.LoadOrder(s, o.Strategy, c)
		if e != nil {
			return in, e
		}
		by := map[string]model.CheckSpec{}
		for _, ch := range c.Checks {
			by[ch.ID] = ch
		}
		ordered = nil
		for _, id := range order {
			ordered = append(ordered, by[id])
		}
		in.Strategy = o.Strategy
	}
	for _, ch := range ordered {
		in.ExecutionOrder = append(in.ExecutionOrder, ch.ID)
	}
	for _, spec := range ordered {
		cr := model.CheckResult{Parser: spec.Parser, ID: spec.ID, Required: spec.Required, Candidate: candidate.ID, SpecDigest: model.Hash(spec), Outcome: "ERROR", EvidenceSource: "controller_observed_process; repository-defined-check"}
		if ctx.Err() != nil {
			cr.Meaning = "verification cancelled before check"
			in.Checks = append(in.Checks, cr)
			continue
		}
		job := filepath.Join(s.Root, "checks", in.ID, spec.ID)
		work := filepath.Join(job, "work")
		if e := source.Materialize(s, candidate, work); e != nil {
			cr.Meaning = "materialize: " + e.Error()
			in.Checks = append(in.Checks, cr)
			continue
		}
		// A preexisting report could be a fabricated stale result. Require generation.
		if spec.ReportPath != "" {
			rp := filepath.Join(work, filepath.FromSlash(spec.ReportPath))
			if _, e := os.Lstat(rp); !os.IsNotExist(e) {
				cr.Meaning = "refusing a preexisting or inaccessible test report"
				in.Checks = append(in.Checks, cr)
				continue
			}
			if e := store.PrivateDir(filepath.Dir(rp)); e != nil {
				cr.Meaning = e.Error()
				in.Checks = append(in.Checks, cr)
				continue
			}
		}
		duration, _ := config.Duration(spec.Timeout)
		run, e := execution.Run(ctx, execution.Options{Dir: work, OutputDir: filepath.Join(job, "output"), Argv: spec.Argv, Timeout: duration, PassEnv: spec.PassEnv, Mode: o.Mode, Image: o.Image})
		if e != nil {
			cr.Meaning = "execution infrastructure: " + e.Error()
			in.Checks = append(in.Checks, cr)
			continue
		}
		cr.Process = run.Process
		cr.Executable = run.Executable
		cr.ExecutableSHA256 = run.ExecutableSHA256
		if _, e = s.Blob(run.Stdout); e != nil {
			return in, e
		}
		if _, e = s.Blob(run.Stderr); e != nil {
			return in, e
		}
		var report []byte
		if spec.ReportPath != "" {
			b, _, e := source.ReadRegular(work, spec.ReportPath, execution.OutputLimit)
			if e != nil {
				cr.Meaning = "report unavailable or unsafe: " + e.Error()
				in.Checks = append(in.Checks, cr)
				continue
			}
			report = b
			cr.ReportSHA256, e = s.Blob(report)
			if e != nil {
				return in, e
			}
		}
		parsed := CapFormalScope(spec.Scope, Interpret(spec, run.Process, run.Stdout, report))
		cr.Outcome = parsed.Outcome
		cr.Meaning = parsed.Meaning
		cr.Tests = parsed.Tests
		cr.Skipped = parsed.Skipped
		cr.Property = spec.Property
		cr.Scope = spec.Scope
		cr.Assumptions = spec.Assumptions
		for _, f := range parsed.Findings {
			f.Evidence = cr.ReportSHA256
			cr.Findings = append(cr.Findings, f)
			in.Findings = append(in.Findings, f)
		}
		if e := source.InputsUnchanged(candidate, work); e != nil {
			cr.Outcome = "ERROR"
			cr.Meaning = e.Error() + "; result does not apply to the original input"
		}
		in.Checks = append(in.Checks, cr)
		if e := s.Put("investigation", in.ID, in, "check.completed"); e != nil {
			return in, e
		}
	}
	in.FinishedAt = model.Now()
	Decide(c, &in)
	if e := s.Put("investigation", in.ID, in, "investigation.completed"); e != nil {
		return in, e
	}
	return in, nil
}
func Review(s *store.Store, id, note string) (model.Approval, error) {
	var in model.Investigation
	if e := s.Get("investigation", id, &in); e != nil {
		return model.Approval{}, e
	}
	if in.Decision != "ACCEPTED" && in.Decision != "REVIEW_REQUIRED" {
		return model.Approval{}, fmt.Errorf("cannot approve %s; required verification is not satisfied", in.Decision)
	}
	if note == "" {
		return model.Approval{}, errors.New("review note is required")
	}
	a := model.Approval{Schema: model.Schema, ID: model.ID("review"), InvestigationID: in.ID, Candidate: in.Candidate, ConfigDigest: in.ConfigDigest, At: model.Now(), Note: note, Kind: "local-review", Authority: "same-user assertion; does not alter checks, grant merge access, or constitute protected approval"}
	return a, s.Put("approval", a.ID, a, "review.recorded")
}
