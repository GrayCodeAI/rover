package assurance

import (
	"context"
	"encoding/json"
	"github.com/GrayCodeAI/rover/internal/model"
	"github.com/GrayCodeAI/rover/internal/source"
	"github.com/GrayCodeAI/rover/internal/testutil"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func commandConfig(script string) model.Config {
	return model.Config{Schema: model.Schema, Checks: []model.CheckSpec{{ID: "required", Argv: []string{"/bin/sh", "-c", script}, Timeout: "2s", Required: true, Parser: "exit-code"}}}
}
func TestVerificationIntegration(t *testing.T) {
	for _, tc := range []struct{ name, script, want string }{{"pass", `test "$(cat value)" = good`, "ACCEPTED"}, {"fail", "exit 7", "BLOCKED"}, {"mutated-input", "printf changed > value", "INCONCLUSIVE"}, {"no-report", "true", "INCONCLUSIVE"}} {
		t.Run(tc.name, func(t *testing.T) {
			repo, s := testutil.Repo(t, map[string]string{"value": "good"})
			snap, e := source.Capture(context.Background(), s, repo, "HEAD", false)
			if e != nil {
				t.Fatal(e)
			}
			cfg := commandConfig(tc.script)
			if tc.name == "no-report" {
				cfg.Checks[0].Parser = "junit"
				cfg.Checks[0].MinTests = 1
				cfg.Checks[0].ReportPath = ".rover-results/report.xml"
			}
			in, e := Verify(context.Background(), s, snap, snap, cfg, Options{Mode: "local-advisory", AllowLocal: true, PolicySource: "explicit-test"})
			if e != nil {
				t.Fatal(e)
			}
			if in.Decision != tc.want {
				t.Fatalf("%s %+v", in.Decision, in.Checks)
			}
			var saved model.Investigation
			if e = s.Get("investigation", in.ID, &saved); e != nil || saved.Candidate != snap.ID {
				t.Fatal(e)
			}
			if e = s.Integrity(); e != nil {
				t.Fatal(e)
			}
		})
	}
}
func TestBaselinePolicyNotCandidatePolicy(t *testing.T) {
	approved := commandConfig(`test "$(cat value)" = good`)
	b, _ := json.Marshal(approved)
	repo, s := testutil.Repo(t, map[string]string{"value": "bad", ".rover/config.json": string(b)})
	base, e := source.Capture(context.Background(), s, repo, "HEAD", false)
	if e != nil {
		t.Fatal(e)
	}
	weakened := commandConfig("true")
	b, _ = json.Marshal(weakened)
	testutil.Write(t, repo, ".rover/config.json", string(b))
	candidate, e := source.Capture(context.Background(), s, repo, "WORKTREE", false)
	if e != nil {
		t.Fatal(e)
	}
	loaded, origin, e := LoadConfig(s, base, "")
	if e != nil {
		t.Fatal(e)
	}
	in, e := Verify(context.Background(), s, base, candidate, loaded, Options{Mode: "local-advisory", AllowLocal: true, PolicySource: origin})
	if e != nil || in.Decision != "BLOCKED" || len(in.Findings) == 0 {
		t.Fatal(e, in)
	}
	if _, e = Review(s, in.ID, "ignore failures"); e == nil {
		t.Fatal("failed checks approved")
	}
}
func TestPreexistingReportRejected(t *testing.T) {
	repo, s := testutil.Repo(t, map[string]string{"value": "good", "fake.xml": "<testsuite><testcase/></testsuite>"})
	snap, e := source.Capture(context.Background(), s, repo, "HEAD", false)
	if e != nil {
		t.Fatal(e)
	}
	c := commandConfig("true")
	c.Checks[0].Parser = "junit"
	c.Checks[0].MinTests = 1
	c.Checks[0].ReportPath = "fake.xml"
	in, e := Verify(context.Background(), s, snap, snap, c, Options{Mode: "local-advisory", AllowLocal: true})
	if e != nil || in.Decision != "INCONCLUSIVE" {
		t.Fatal(e, in)
	}
}
func TestExplicitGrantRequired(t *testing.T) {
	repo, s := testutil.Repo(t, map[string]string{"value": "good"})
	snap, e := source.Capture(context.Background(), s, repo, "HEAD", false)
	if e != nil {
		t.Fatal(e)
	}
	marker := filepath.Join(t.TempDir(), "marker")
	c := commandConfig("touch " + marker)
	_, e = Verify(context.Background(), s, snap, snap, c, Options{Mode: "local-advisory"})
	if e == nil {
		t.Fatal("missing grant accepted")
	}
	if _, e = os.Stat(marker); !os.IsNotExist(e) {
		t.Fatal("executed without grant")
	}
}
func TestReplayRetainedInvestigation(t *testing.T) {
	repo, s := testutil.Repo(t, map[string]string{"value": "good"})
	base, e := source.Capture(context.Background(), s, repo, "HEAD", false)
	if e != nil {
		t.Fatal(e)
	}
	cfg := commandConfig(`test "$(cat value)" = good`)
	orig, e := Verify(context.Background(), s, base, base, cfg, Options{Mode: "local-advisory", AllowLocal: true, PolicySource: "test-origin"})
	if e != nil || orig.Decision != "ACCEPTED" {
		t.Fatal(e, orig)
	}
	replayed, e := Replay(context.Background(), s, orig.ID, Options{Mode: "local-advisory", AllowLocal: true})
	if e != nil || replayed.Decision != "ACCEPTED" {
		t.Fatal(e, replayed)
	}
	if replayed.Base != orig.Base || replayed.Candidate != orig.Candidate || replayed.ConfigDigest != orig.ConfigDigest {
		t.Fatal("replay changed retained snapshots/config")
	}
	if replayed.ID == orig.ID {
		t.Fatal("replay must produce a new investigation ID")
	}
	if !strings.HasPrefix(replayed.PolicySource, "replay:") {
		t.Fatal("replay policy source not tagged", replayed.PolicySource)
	}
	var stillOrig model.Investigation
	if e = s.Get("investigation", orig.ID, &stillOrig); e != nil {
		t.Fatal(e)
	}
	if stillOrig.Decision != "ACCEPTED" {
		t.Fatal("original investigation was modified", stillOrig)
	}
}
