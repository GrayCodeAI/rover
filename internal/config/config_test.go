package config

import (
	"encoding/json"
	"github.com/GrayCodeAI/rover/internal/model"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func valid() model.Config {
	return model.Config{Schema: model.Schema, Checks: []model.CheckSpec{{ID: "unit", Argv: []string{"go", "test", "-json", "./..."}, Timeout: "1m", Parser: "go-test-json", Required: true, MinTests: 1}}}
}
func TestDecodeStrict(t *testing.T) {
	cases := []struct {
		name, b string
		wantErr bool
	}{{"valid", `{"schema":"rover/v1alpha1","checks":[],"policy":{"require_review":true}}`, false}, {"unknown-field", `{"schema":"rover/v1alpha1","cheks":[]}`, true}, {"duplicate-root", `{"schema":"a","schema":"b"}`, true}, {"duplicate-nested", `{"policy":{"require_review":true,"require_review":false}}`, true}, {"two-documents", `{} {}`, true}, {"malformed", `{"x":`, true}, {"empty", "", true}, {"oversized", strings.Repeat(" ", MaxBytes+1), true}, {"deep-nesting", strings.Repeat("[", 150) + strings.Repeat("]", 150), true}}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			var v model.Config
			e := Decode([]byte(c.b), &v)
			if (e != nil) != c.wantErr {
				t.Fatalf("err=%v", e)
			}
		})
	}
}
func TestValidate(t *testing.T) {
	tests := []struct {
		name string
		mut  func(*model.Config)
	}{
		{"schema", func(c *model.Config) { c.Schema = "future" }}, {"duplicate-id", func(c *model.Config) { c.Checks = append(c.Checks, c.Checks[0]) }},
		{"unsafe-id", func(c *model.Config) { c.Checks[0].ID = "../check" }}, {"empty-argv", func(c *model.Config) { c.Checks[0].Argv = nil }},
		{"NUL-argv", func(c *model.Config) { c.Checks[0].Argv = []string{"sh", "a\x00b"} }}, {"zero-timeout", func(c *model.Config) { c.Checks[0].Timeout = "0s" }},
		{"excessive-timeout", func(c *model.Config) { c.Checks[0].Timeout = "25h" }}, {"unknown-parser", func(c *model.Config) { c.Checks[0].Parser = "magic" }},
		{"no-min-tests", func(c *model.Config) { c.Checks[0].MinTests = 0 }}, {"unsafe-env", func(c *model.Config) { c.Checks[0].PassEnv = []string{"x=y"} }},
		{"bad-glob", func(c *model.Config) { c.Policy.ReviewPaths = []string{"../auth/**"} }}, {"junit-traversal", func(c *model.Config) { c.Checks[0].Parser = "junit"; c.Checks[0].ReportPath = "../../result.xml" }},
	}
	if e := Validate(valid()); e != nil {
		t.Fatal(e)
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			c := valid()
			tc.mut(&c)
			if e := Validate(c); e == nil {
				t.Fatal("wanted validation error")
			}
		})
	}
}
func TestGlob(t *testing.T) {
	for _, tc := range []struct {
		p, s string
		want bool
	}{{"auth/**", "auth/a/b.go", true}, {"**/*_test.go", "main_test.go", true}, {"**/*_test.go", "a/b_test.go", true}, {"src/*.go", "src/a/b.go", false}, {"x?.go", "x1.go", true}} {
		re, e := Glob(tc.p)
		if e != nil {
			t.Fatal(e)
		}
		if re.MatchString(tc.s) != tc.want {
			t.Errorf("%s %s", tc.p, tc.s)
		}
	}
}
func TestTaskRejectsUnsupportedDAG(t *testing.T) {
	v := model.TaskSpec{Schema: model.Schema, Objective: "fix", Repository: ".", Base: "HEAD", Argv: []string{"true"}, Timeout: "1m", DependsOn: []string{"upstream"}}
	if ValidateTask(v) == nil {
		t.Fatal("DAG unexpectedly supported")
	}
	v.DependsOn = nil
	if e := ValidateTask(v); e != nil {
		t.Fatal(e)
	}
}
func FuzzDecode(f *testing.F) {
	f.Add([]byte(`{"schema":"rover/v1alpha1","checks":[]}`))
	f.Add([]byte(`{"x":{"x":1,"x":2}}`))
	f.Fuzz(func(t *testing.T, b []byte) {
		if len(b) > MaxBytes {
			return
		}
		var c model.Config
		if Decode(b, &c) == nil {
			encoded, e := json.Marshal(c)
			if e != nil {
				t.Fatal(e)
			}
			var round model.Config
			if e = Decode(encoded, &round); e != nil {
				t.Fatal(e)
			}
		}
	})
}

func TestInvalidUTF8Path(t *testing.T) {
	if Relative(string([]byte{255})) {
		t.Fatal("invalid UTF-8 accepted")
	}
}

func TestUnicodeGlob(t *testing.T) {
	re, e := Glob("कूट/**/é*.go")
	if e != nil {
		t.Fatal(e)
	}
	if !re.MatchString("कूट/éx.go") || !re.MatchString("कूट/src/éx.go") || re.MatchString("other/éx.go") {
		t.Fatal("incorrect Unicode path match")
	}
}

func TestExplicitAuthorityFields(t *testing.T) {
	for _, raw := range []string{
		`{"schema":"rover/v1alpha1","checks":[],"policy":{}}`,
		`{"schema":"rover/v1alpha1","checks":[],"policy":{"require_review":null}}`,
		`{"schema":"rover/v1alpha1","checks":[],"policy":{"Require_Review":false}}`,
		`{"schema":"rover/v1alpha1","checks":[],"policy":{"require_review":true},"Schema":"future"}`,
		`{"schema":"rover/v1alpha1","checks":[{"id":"one","argv":["true"],"timeout":"1s","parser":"exit-code"}],"policy":{"require_review":true}}`,
	} {
		var c model.Config
		if Decode([]byte(raw), &c) == nil {
			t.Fatalf("accepted ambiguous config: %s", raw)
		}
	}
	var task model.TaskSpec
	if Decode([]byte(`{"schema":"rover/v1alpha1","objective":"x","repository":".","base":"HEAD","argv":["true"],"timeout":"1s"}`), &task) == nil {
		t.Fatal("implicit auto_verify accepted")
	}
}

func TestRepositoryExamples(t *testing.T) {
	for _, p := range []string{"../../.rover/config.json", "../../examples/fixture/.rover/config.json"} {
		var c model.Config
		if e := Read(p, &c); e != nil {
			t.Fatal(p, e)
		}
		if e := Validate(c); e != nil {
			t.Fatal(p, e)
		}
	}
	names, e := filepath.Glob("../../examples/task*.json")
	if e != nil || len(names) == 0 {
		t.Fatal(e, "missing examples")
	}
	for _, p := range names {
		var task model.TaskSpec
		b, e := os.ReadFile(p)
		if e != nil {
			t.Fatal(e)
		}
		if e = Decode(b, &task); e != nil {
			t.Fatal(p, e)
		}
		if e = ValidateTask(task); e != nil {
			t.Fatal(p, e)
		}
	}
}
