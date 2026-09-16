package assurance

import (
	"github.com/GrayCodeAI/rover/internal/model"
	"strings"
	"testing"
)

const goodGo = `{"Action":"start","Package":"p"}
{"Action":"run","Package":"p","Test":"TestA"}
{"Action":"pass","Package":"p","Test":"TestA"}
{"Action":"pass","Package":"p"}
`

func TestParsers(t *testing.T) {
	cases := []struct {
		name, parser, out, report string
		min, exit                 int
		want                      string
	}{
		{"exit0", "exit-code", "", "", 0, 0, "PASS"}, {"exit7", "exit-code", "", "", 0, 7, "FAIL"},
		{"go-success", "go-test-json", goodGo, "", 1, 0, "PASS"}, {"go-empty", "go-test-json", "", "", 1, 0, "INCONCLUSIVE"},
		{"go-malformed", "go-test-json", "green!", "", 1, 0, "ERROR"}, {"go-zero", "go-test-json", `{"Action":"pass","Package":"p"}`, "", 1, 0, "INCONCLUSIVE"},
		{"go-missing-run", "go-test-json", `{"Action":"pass","Package":"p","Test":"TestA"}`, "", 1, 0, "ERROR"},
		{"go-incomplete", "go-test-json", `{"Action":"run","Package":"p","Test":"TestA"}`, "", 1, 0, "INCONCLUSIVE"},
		{"junit-pass", "junit", "", `<testsuite><testcase name="one"/></testsuite>`, 1, 0, "PASS"},
		{"junit-skipped", "junit", "", `<testsuite><testcase><skipped/></testcase></testsuite>`, 1, 0, "INCONCLUSIVE"},
		{"junit-forged-count", "junit", "", `<testsuite tests="999"/>`, 1, 0, "INCONCLUSIVE"},
		{"junit-summary-failure", "junit", "", `<testsuite failures="1"><testcase name="one"/></testsuite>`, 1, 0, "FAIL"},
		{"junit-fail", "junit", "", `<testsuites><testsuite><testcase><failure/></testcase></testsuite></testsuites>`, 1, 0, "FAIL"},
		{"junit-malformed", "junit", "", `<testsuite`, 1, 0, "ERROR"}, {"junit-foreign-root", "junit", "", `<html/>`, 1, 0, "ERROR"},
		{"junit-trailing", "junit", "", `<testsuite><testcase/></testsuite><anything/>`, 1, 0, "ERROR"},
		{"junit-missing", "junit", "", "", 1, 0, "INCONCLUSIVE"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			s := model.CheckSpec{Parser: tc.parser, MinTests: tc.min}
			p := model.ProcessResult{ExitCode: tc.exit}
			got := Interpret(s, p, []byte(tc.out), []byte(tc.report))
			if got.Outcome != tc.want {
				t.Fatalf("%+v want %s", got, tc.want)
			}
		})
	}
}
func TestIncompleteExecutionNeverPasses(t *testing.T) {
	for _, tc := range []struct {
		name string
		p    model.ProcessResult
		want string
	}{{"timeout", model.ProcessResult{TimedOut: true}, "ERROR"}, {"cancelled", model.ProcessResult{Cancelled: true}, "ERROR"}, {"truncated", model.ProcessResult{Truncated: true}, "INCONCLUSIVE"}, {"start-error", model.ProcessResult{ExitCode: -1, Error: "missing executable"}, "ERROR"}, {"cleanup-error", model.ProcessResult{ExitCode: 0, Error: "cleanup uncertain"}, "ERROR"}} {
		t.Run(tc.name, func(t *testing.T) {
			r := Interpret(model.CheckSpec{Parser: "exit-code"}, tc.p, nil, nil)
			if r.Outcome != tc.want {
				t.Fatal(r)
			}
		})
	}
}
func TestFormalScopeHeuristic(t *testing.T) {
	cases := []struct {
		name, scope string
		in, want    Parsed
	}{
		{"formal-pass-capped", "formal", Parsed{Outcome: "PASS"}, Parsed{Outcome: "INCONCLUSIVE"}},
		{"formal-fail-unchanged", "formal", Parsed{Outcome: "FAIL"}, Parsed{Outcome: "FAIL"}},
		{"formal-inconclusive-unchanged", "formal", Parsed{Outcome: "INCONCLUSIVE"}, Parsed{Outcome: "INCONCLUSIVE"}},
		{"advisory-pass-unchanged", "advisory", Parsed{Outcome: "PASS"}, Parsed{Outcome: "PASS"}},
		{"empty-scope-pass-unchanged", "", Parsed{Outcome: "PASS"}, Parsed{Outcome: "PASS"}},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			got := CapFormalScope(tc.scope, tc.in)
			if got.Outcome != tc.want.Outcome {
				t.Fatalf("CapFormalScope(%q, PASS) = %s, want %s", tc.scope, got.Outcome, tc.want.Outcome)
			}
		})
	}
	capped := CapFormalScope("formal", Parsed{Outcome: "PASS", Meaning: "advisory meaning"})
	if capped.Outcome != "INCONCLUSIVE" || !strings.Contains(capped.Meaning, "not a machine-checkable proof") {
		t.Fatalf("capped result must name the missing proof artifact, got %+v", capped)
	}
	if CapFormalScope("advisory", Parsed{Outcome: "PASS", Meaning: "x"}).Meaning != "x" {
		t.Fatal("advisory scope must not be rewritten")
	}
}

func TestPolicyCompleteness(t *testing.T) {
	spec := model.CheckSpec{ID: "a", Argv: []string{"true"}, Timeout: "1s", Required: true, Parser: "exit-code"}
	cfg := model.Config{Schema: model.Schema, Checks: []model.CheckSpec{spec}}
	for _, tc := range []struct {
		name, outcome, cand, digest string
		want                        string
	}{{"pass", "PASS", "s", model.Hash(spec), "ACCEPTED"}, {"fail", "FAIL", "s", model.Hash(spec), "BLOCKED"}, {"error", "ERROR", "s", model.Hash(spec), "INCONCLUSIVE"}, {"wrong-candidate", "PASS", "other", model.Hash(spec), "INCONCLUSIVE"}, {"wrong-spec", "PASS", "s", "wrong", "INCONCLUSIVE"}} {
		t.Run(tc.name, func(t *testing.T) {
			i := model.Investigation{Candidate: "s", Checks: []model.CheckResult{{ID: "a", Outcome: tc.outcome, Candidate: tc.cand, SpecDigest: tc.digest}}}
			Decide(cfg, &i)
			if i.Decision != tc.want {
				t.Fatal(i.Decision)
			}
		})
	}
	i := model.Investigation{Candidate: "s"}
	Decide(cfg, &i)
	if i.Decision != "INCONCLUSIVE" {
		t.Fatal(i.Decision)
	}
	Decide(model.Config{}, &i)
	if i.Decision != "INCONCLUSIVE" {
		t.Fatal(i.Decision)
	}
}
func FuzzResultParser(f *testing.F) {
	f.Add([]byte(goodGo))
	f.Add([]byte(`{bad`))
	f.Fuzz(func(t *testing.T, b []byte) {
		if len(b) > 65536 {
			return
		}
		_ = Interpret(model.CheckSpec{Parser: "go-test-json", MinTests: 1}, model.ProcessResult{ExitCode: 0}, b, nil)
	})
}
