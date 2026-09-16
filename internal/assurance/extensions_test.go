package assurance

import (
	"context"
	"encoding/json"
	"github.com/GrayCodeAI/rover/internal/model"
	"github.com/GrayCodeAI/rover/internal/source"
	"github.com/GrayCodeAI/rover/internal/testutil"
	"testing"
)

func TestSARIF(t *testing.T) {
	good := []byte(`{"version":"2.1.0","runs":[{"tool":{"driver":{"name":"scanner"}},"results":[]}]}`)
	if p := sarif(good, 0, ""); p.Outcome != "PASS" {
		t.Fatal(p)
	}
	for _, bad := range []string{`{}`, `{"version":"2.1.0","runs":[]}`, `{"version":"2.1.0","runs":[{"tool":{"driver":{"name":"s"}}}]}`} {
		if p := sarif([]byte(bad), 0, ""); p.Outcome == "PASS" {
			t.Fatal(p)
		}
	}
	p := sarif([]byte(`{"version":"2.1.0","runs":[{"tool":{"driver":{"name":"s"}},"results":[{"ruleId":"S1","level":"error","message":{"text":"finding"},"locations":[{"physicalLocation":{"artifactLocation":{"uri":"../../outside"},"region":{"startLine":5}}}]}]}]}`), 0, "warning")
	if p.Outcome != "FAIL" || len(p.Findings) != 1 || p.Findings[0].Path != "" {
		t.Fatal(p)
	}
}

const regressionProgram = `import pathlib,sys,xml.etree.ElementTree as E
ok=pathlib.Path('value.txt').read_text()=='good'
suite=E.Element('testsuite',tests='1',failures='0' if ok else '1',errors='0',skipped='0')
case=E.SubElement(suite,'testcase',name='denies_reuse',classname='Auth')
if not ok:
    E.SubElement(case,'failure',message='EXPECTED_AUTH_DENIAL').text='token reuse accepted'
pathlib.Path('.rover-results').mkdir(exist_ok=True)
E.ElementTree(suite).write('.rover-results/junit.xml')
sys.exit(0 if ok else 1)
`

func TestCounterfactualAndMutation(t *testing.T) {
	cfg := model.Config{Schema: model.Schema, Checks: []model.CheckSpec{{ID: "regression", Argv: []string{"python3", "tests/test_regression.py"}, Parser: "junit", ReportPath: ".rover-results/junit.xml", MinTests: 1, Timeout: "2s", Required: true}}, Policy: model.Policy{RequireReview: true}}
	b, _ := json.Marshal(cfg)
	repo, s := testutil.Repo(t, map[string]string{"value.txt": "bad", ".rover/config.json": string(b)})
	base, e := source.Capture(context.Background(), s, repo, "HEAD", false)
	if e != nil {
		t.Fatal(e)
	}
	testutil.Write(t, repo, "value.txt", "good")
	testutil.Write(t, repo, "tests/test_regression.py", regressionProgram)
	testutil.Commit(t, repo)
	cand, e := source.Capture(context.Background(), s, repo, "HEAD", false)
	if e != nil {
		t.Fatal(e)
	}
	sp := RegressionSpec{Schema: model.Schema, CheckID: "regression", TestPaths: []string{"tests/test_regression.py"}, TestID: "Auth::denies_reuse", FailureContains: "EXPECTED_AUTH_DENIAL"}
	o := Options{Mode: "local-advisory", AllowLocal: true}
	r, e := ProveRegression(context.Background(), s, base, cand, cfg, sp, o)
	if e != nil || r.Assessment != "SUPPORTED_WITHIN_SCOPE" {
		t.Fatalf("%+v %v", r, e)
	}
	sp.FailureContains = "another failure"
	r, e = ProveRegression(context.Background(), s, base, cand, cfg, sp, o)
	if e != nil || r.Assessment != "INCONCLUSIVE" {
		t.Fatalf("%+v %v", r, e)
	}
	mr, e := Mutate(context.Background(), s, cand, cand, cfg, MutationSpec{Schema: model.Schema, CheckIDs: []string{"regression"}, Mutations: []Mutation{{ID: "broken", Path: "value.txt", Before: "good", After: "bad"}}}, o)
	if e != nil || len(mr.Results) != 1 || mr.Results[0].Result != "KILLED" {
		t.Fatalf("%+v %v", mr, e)
	}
	replay, e := Replay(context.Background(), s, mr.BaselineInvestigation, o)
	if e != nil || replay.Candidate != cand.ID || replay.ID == mr.BaselineInvestigation {
		t.Fatalf("%+v %v", replay, e)
	}
}
func TestCounterfactualIdentityAmbiguity(t *testing.T) {
	b := []byte(`<testsuites><testsuite><testcase name="x" classname="A"><failure message="expected"/></testcase><testcase name="x" classname="B"><failure message="expected"/></testcase></testsuite></testsuites>`)
	if _, ok := testEvidence("junit", b, "x"); ok {
		t.Fatal("ambiguous test name accepted")
	}
	o, ok := testEvidence("junit", b, "A::x")
	if !ok || o.Status != "FAIL" {
		t.Fatal(o, ok)
	}
	o, ok = testEvidence("junit", []byte(`<testsuite><testcase name="x"><error message="import error"/></testcase></testsuite>`), "x")
	if !ok || o.Status != "ERROR" {
		t.Fatal(o, ok)
	}
}
