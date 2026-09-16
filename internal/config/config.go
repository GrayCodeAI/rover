// Package config parses explicit, versioned JSON. Unknown fields and duplicate
// keys are errors: a misspelled required check must never disappear silently.
package config

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path"
	"path/filepath"
	"regexp"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/GrayCodeAI/rover/internal/model"
)

const MaxBytes = 1 << 20

var envName = regexp.MustCompile(`^[A-Za-z_][A-Za-z0-9_]*$`)

func Decode(b []byte, dst any) error {
	if len(b) > MaxBytes {
		return errors.New("configuration exceeds 1 MiB")
	}
	d := json.NewDecoder(bytes.NewReader(b))
	if err := walk(d); err != nil {
		return err
	}
	if _, err := d.Token(); err != io.EOF {
		return errors.New("expected exactly one JSON document")
	}
	d = json.NewDecoder(bytes.NewReader(b))
	d.DisallowUnknownFields()
	if err := d.Decode(dst); err != nil {
		return fmt.Errorf("invalid configuration: %w", err)
	}
	// encoding/json accepts case-insensitive struct fields and null for scalar
	// fields. Admission flags must be explicit, canonical, non-null values.
	switch dst.(type) {
	case *model.Config:
		o, err := exactObject(b, []string{"schema", "checks", "policy"}, nil)
		if err != nil {
			return err
		}
		if _, err = exactObject(o["policy"], []string{"require_review"}, []string{"review_paths"}); err != nil {
			return err
		}
		var checks []json.RawMessage
		if err = json.Unmarshal(o["checks"], &checks); err != nil {
			return err
		}
		for _, raw := range checks {
			if _, err = exactObject(raw, []string{"id", "argv", "timeout", "required", "parser"}, []string{"min_tests", "report_path", "pass_env", "fail_level", "property", "scope", "assumptions"}); err != nil {
				return err
			}
		}
	case *model.TaskSpec:
		if _, err := exactObject(b, []string{"schema", "objective", "repository", "base", "argv", "timeout", "auto_verify"}, []string{"pass_env", "depends_on", "config_path", "agent", "agent_options", "interactive", "max_attempts", "repair_argv", "reservations", "initial_snapshot"}); err != nil {
			return err
		}
	}
	return nil
}
func exactObject(b []byte, required, optional []string) (map[string]json.RawMessage, error) {
	var obj map[string]json.RawMessage
	if err := json.Unmarshal(b, &obj); err != nil || obj == nil {
		return nil, errors.New("expected configuration object")
	}
	allowed := map[string]bool{}
	for _, k := range optional {
		allowed[k] = true
	}
	for _, k := range required {
		allowed[k] = true
		v, ok := obj[k]
		if !ok || bytes.Equal(bytes.TrimSpace(v), []byte("null")) {
			return nil, fmt.Errorf("field %q must be explicitly present and non-null", k)
		}
	}
	for k := range obj {
		if !allowed[k] {
			return nil, fmt.Errorf("unknown or noncanonical field %q", k)
		}
	}
	return obj, nil
}
func walk(d *json.Decoder) error {
	t, e := d.Token()
	if e != nil {
		return e
	}
	delim, ok := t.(json.Delim)
	if !ok {
		return nil
	}
	switch delim {
	case '{':
		seen := map[string]bool{}
		for d.More() {
			k, e := d.Token()
			if e != nil {
				return e
			}
			s, ok := k.(string)
			if !ok {
				return errors.New("expected object key")
			}
			if seen[s] {
				return fmt.Errorf("duplicate JSON key %q", s)
			}
			seen[s] = true
			if e := walk(d); e != nil {
				return e
			}
		}
	case '[':
		for d.More() {
			if e := walk(d); e != nil {
				return e
			}
		}
	default:
		return errors.New("unexpected closing delimiter")
	}
	_, e = d.Token()
	return e
}
func Read(path string, dst any) error {
	st, e := os.Lstat(path)
	if e != nil {
		return e
	}
	if !st.Mode().IsRegular() {
		return errors.New("config must be a regular file, not a symlink")
	}
	f, e := os.Open(path)
	if e != nil {
		return e
	}
	defer f.Close()
	b, e := io.ReadAll(io.LimitReader(f, MaxBytes+1))
	if e != nil {
		return e
	}
	return Decode(b, dst)
}
func Duration(s string) (time.Duration, error) {
	d, e := time.ParseDuration(s)
	if e != nil || d < time.Millisecond || d > 24*time.Hour {
		return 0, fmt.Errorf("duration %q must be between 1ms and 24h", s)
	}
	return d, nil
}
func Argv(v []string) error {
	if len(v) == 0 || len(v) > 128 || strings.TrimSpace(v[0]) == "" {
		return errors.New("argv must contain an executable and at most 128 arguments")
	}
	for _, s := range v {
		if strings.ContainsRune(s, 0) || len(s) > 65536 {
			return errors.New("argv contains NUL or oversized argument")
		}
	}
	return nil
}
func Env(v []string) error {
	for _, s := range v {
		if !envName.MatchString(s) {
			return fmt.Errorf("invalid environment variable name %q", s)
		}
	}
	return nil
}
func Relative(p string) bool {
	return utf8.ValidString(p) && p != "" && p != "." && !path.IsAbs(p) && path.Clean(p) == p && !strings.Contains(p, "\\") && !strings.ContainsRune(p, 0) && p != ".." && !strings.HasPrefix(p, "../")
}
func Validate(c model.Config) error {
	if c.Schema != model.Schema {
		return fmt.Errorf("unsupported schema %q", c.Schema)
	}
	seen := map[string]bool{}
	if len(c.Checks) > 128 {
		return errors.New("at most 128 checks supported")
	}
	for _, x := range c.Checks {
		if !model.ValidID(x.ID) || seen[x.ID] {
			return fmt.Errorf("invalid or duplicated check id %q", x.ID)
		}
		seen[x.ID] = true
		if e := Argv(x.Argv); e != nil {
			return fmt.Errorf("check %s: %w", x.ID, e)
		}
		if _, e := Duration(x.Timeout); e != nil {
			return e
		}
		if e := Env(x.PassEnv); e != nil {
			return e
		}
		switch x.Parser {
		case "exit-code":
			if x.MinTests != 0 || x.ReportPath != "" {
				return errors.New("exit-code parser cannot claim test counts or parse a report")
			}
		case "go-test-json":
			if x.MinTests < 1 || x.ReportPath != "" {
				return errors.New("go-test-json requires min_tests >= 1 and no report_path")
			}
		case "sarif":
			if x.MinTests != 0 || !Relative(x.ReportPath) {
				return errors.New("sarif requires report_path and no test count")
			}
			switch x.FailLevel {
			case "", "error", "warning", "note":
			default:
				return errors.New("invalid SARIF fail_level")
			}
		case "junit":
			if x.MinTests < 1 || !Relative(x.ReportPath) {
				return errors.New("junit requires min_tests >= 1 and safe relative report_path")
			}
		default:
			return fmt.Errorf("unsupported parser %q", x.Parser)
		}
	}
	for _, p := range c.Policy.ReviewPaths {
		if _, e := Glob(p); e != nil {
			return e
		}
	}
	return nil
}
func ValidateTask(t model.TaskSpec) error {
	if t.Schema != model.Schema {
		return errors.New("unsupported task schema")
	}
	if strings.TrimSpace(t.Objective) == "" || len(t.Objective) > 32768 {
		return errors.New("objective must be nonempty and <= 32768 bytes")
	}
	if t.Repository == "" || t.Base == "" {
		return errors.New("task repository and base are required")
	}
	if t.Agent == "" || t.Agent == "generic-headless" || t.Agent == "generic-pty" {
		if e := Argv(t.Argv); e != nil {
			return e
		}
	} else {
		if t.Agent != "codex-exec" && t.Agent != "claude-print" {
			return errors.New("unknown agent adapter")
		}
		if len(t.Argv) != 0 || t.Interactive {
			return errors.New("native headless adapter requires empty argv and interactive=false")
		}
	}
	if t.AgentOptions.MaxTurns < 0 || t.AgentOptions.MaxTurns > 1000 {
		return errors.New("invalid max_turns")
	}
	for _, v := range append([]string{t.AgentOptions.Executable, t.AgentOptions.Model}, t.AgentOptions.AllowedTools...) {
		if len(v) > 4096 || strings.ContainsRune(v, 0) {
			return errors.New("invalid agent option")
		}
	}
	if t.MaxAttempts < 0 || t.MaxAttempts > 10 {
		return errors.New("max_attempts must be 1..10; zero defaults to one")
	}
	if t.MaxAttempts > 1 {
		if !t.AutoVerify || t.Interactive || t.Agent == "generic-pty" {
			return errors.New("repair attempts require auto_verify and headless execution")
		}
		if t.Agent == "" || strings.HasPrefix(t.Agent, "generic-") {
			if e := Argv(t.RepairArgv); e != nil {
				return errors.New("generic repair requires explicit repair_argv")
			}
		}
	}
	if t.InitialSnapshot != "" && !model.ValidID(t.InitialSnapshot) {
		return errors.New("invalid input snapshot")
	}
	seen := map[string]bool{}
	for _, k := range t.Reservations {
		if !model.ValidID(k) || len(k) > 96 || seen[k] {
			return errors.New("invalid or duplicate resource")
		}
		seen[k] = true
	}
	if len(seen) > 64 {
		return errors.New("too many reservations")
	}
	if _, e := Duration(t.Timeout); e != nil {
		return e
	}
	if e := Env(t.PassEnv); e != nil {
		return e
	}
	if len(t.DependsOn) > 0 {
		return errors.New("dependency scheduling is not implemented in this alpha; depends_on must be empty")
	}
	return nil
}

// Glob implements only *, ** and ?. Character classes are deliberately not
// supported; invalid patterns fail validation instead of matching nothing.
func Glob(p string) (*regexp.Regexp, error) {
	if !Relative(p) || strings.ContainsAny(p, "[]{}\r\n") {
		return nil, fmt.Errorf("unsupported path pattern %q", p)
	}
	var b strings.Builder
	b.WriteString("^")
	runes := []rune(p)
	for i := 0; i < len(runes); i++ {
		switch runes[i] {
		case '*':
			if i+1 < len(runes) && runes[i+1] == '*' {
				if i+2 < len(runes) && runes[i+2] == '/' {
					b.WriteString("(?:.*/)?")
					i += 2
				} else {
					b.WriteString(".*")
					i++
				}
			} else {
				b.WriteString("[^/]*")
			}
		case '?':
			b.WriteString("[^/]")
		default:
			b.WriteString(regexp.QuoteMeta(string(runes[i])))
		}
	}
	b.WriteString("$")
	return regexp.Compile(b.String())
}
func Suggest(repo string) model.Config {
	c := model.Config{Schema: model.Schema, Checks: []model.CheckSpec{}, Policy: model.Policy{RequireReview: true}}
	exists := func(p string) bool { s, e := os.Lstat(filepath.Join(repo, p)); return e == nil && s.Mode().IsRegular() }
	if exists("go.mod") {
		c.Checks = []model.CheckSpec{{ID: "unit", Argv: []string{"go", "test", "-json", "-count=1", "./..."}, Timeout: "5m", Required: true, Parser: "go-test-json", MinTests: 1}, {ID: "vet", Argv: []string{"go", "vet", "./..."}, Timeout: "5m", Required: true, Parser: "exit-code"}}
	}
	return c
}
