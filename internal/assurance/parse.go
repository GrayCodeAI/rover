// Package assurance interprets observed checks and applies explicit policy.
// An exit code, test outcome, claim assessment and acceptance are distinct.
package assurance

import (
	"bytes"
	"encoding/json"
	"encoding/xml"
	"fmt"
	"io"
	"strings"

	"github.com/GrayCodeAI/rover/internal/model"
)

type Parsed struct {
	Outcome, Meaning string
	Tests, Skipped   int
	Findings         []model.Finding
}

func Interpret(spec model.CheckSpec, p model.ProcessResult, out, report []byte) Parsed {
	if p.TimedOut {
		return Parsed{Outcome: "ERROR", Meaning: "check timed out; verification incomplete"}
	}
	if p.Cancelled {
		return Parsed{Outcome: "ERROR", Meaning: "check cancelled; verification incomplete"}
	}
	if p.Truncated {
		return Parsed{Outcome: "INCONCLUSIVE", Meaning: "output exceeded capture limit; complete evidence unavailable"}
	}
	if p.ExitCode < 0 {
		return Parsed{Outcome: "ERROR", Meaning: "process did not complete normally: " + p.Error}
	}
	if p.ExitCode == 0 && p.Error != "" {
		return Parsed{Outcome: "ERROR", Meaning: p.Error}
	}
	switch spec.Parser {
	case "exit-code":
		if p.ExitCode != 0 {
			return Parsed{Outcome: "FAIL", Meaning: "configured process returned a nonzero exit status"}
		}
		return Parsed{Outcome: "PASS", Meaning: "configured process exited zero; not a claim of test coverage or correctness"}
	case "go-test-json":
		return goTest(out, spec.MinTests, p.ExitCode)
	case "sarif":
		return sarif(report, p.ExitCode, spec.FailLevel)
	case "junit":
		return junit(report, spec.MinTests, p.ExitCode)
	default:
		return Parsed{Outcome: "ERROR", Meaning: "unsupported result parser"}
	}
}

// CapFormalScope enforces the documented boundary that an interpretive
// parser result (exit-code, test/coverage JSON, SARIF, JUnit) is never a
// machine-checkable proof. A scope declared "formal" that only has such an
// interpretive PASS is capped to advisory INCONCLUSIVE with the reasoning
// made explicit; it is never upgraded to PASS.
func CapFormalScope(declared string, p Parsed) Parsed {
	if declared != "formal" || p.Outcome != "PASS" {
		return p
	}
	return Parsed{
		Outcome: "INCONCLUSIVE",
		Meaning: "declared formal scope: an interpretive PASS is not a machine-checkable proof; requires an external formal artifact (e.g. Lean/Kani/TLA+ replay) recorded as evidence",
	}
}
func goTest(b []byte, min, exit int) Parsed {
	type evt struct{ Action, Package, Test, Output string }
	d := json.NewDecoder(bytes.NewReader(b))
	terminal := map[string]string{}
	started := map[string]bool{}
	pkgState := map[string]string{}
	pkgSeen := map[string]bool{}
	count := 0
	failed := false
	for {
		var e evt
		err := d.Decode(&e)
		if err == io.EOF {
			break
		}
		if err != nil {
			return Parsed{Outcome: "ERROR", Meaning: "malformed go test JSON: " + err.Error()}
		}
		count++
		if count > 100000 {
			return Parsed{Outcome: "ERROR", Meaning: "too many test events"}
		}
		if e.Package == "" || e.Action == "" {
			return Parsed{Outcome: "ERROR", Meaning: "test event missing package/action"}
		}
		pkgSeen[e.Package] = true
		switch e.Action {
		case "start", "run", "pause", "cont", "output", "bench", "pass", "fail", "skip":
		default:
			return Parsed{Outcome: "INCONCLUSIVE", Meaning: "unrecognized go test event: " + e.Action}
		}
		if e.Test != "" {
			key := e.Package + "/" + e.Test
			if e.Action == "run" {
				started[key] = true
			}
			if e.Action == "pass" || e.Action == "skip" || e.Action == "fail" {
				if !started[key] {
					return Parsed{Outcome: "ERROR", Meaning: "test terminal event has no corresponding run"}
				}
				if _, ok := terminal[key]; ok {
					return Parsed{Outcome: "ERROR", Meaning: "duplicate terminal test event"}
				}
				terminal[key] = e.Action
			}
		} else if e.Action == "pass" || e.Action == "fail" || e.Action == "skip" {
			pkgState[e.Package] = e.Action
		}
		if e.Action == "fail" {
			failed = true
		}
	}
	r := Parsed{}
	for _, s := range terminal {
		if s == "skip" {
			r.Skipped++
		} else {
			r.Tests++
		}
	}
	if failed || exit != 0 {
		r.Outcome = "FAIL"
		r.Meaning = "go test reported failure or a nonzero command result"
		return r
	}
	if count == 0 {
		r.Outcome = "INCONCLUSIVE"
		r.Meaning = "no test events received"
		return r
	}
	for p := range pkgSeen {
		if pkgState[p] == "" {
			r.Outcome = "INCONCLUSIVE"
			r.Meaning = "missing package completion: " + p
			return r
		}
	}
	for t := range started {
		if terminal[t] == "" {
			r.Outcome = "INCONCLUSIVE"
			r.Meaning = "started test did not finish: " + t
			return r
		}
	}
	if r.Tests < min {
		r.Outcome = "INCONCLUSIVE"
		r.Meaning = fmt.Sprintf("expected at least %d non-skipped tests; observed %d", min, r.Tests)
		return r
	}
	r.Outcome = "PASS"
	r.Meaning = "identified non-skipped tests completed; adequate requirement coverage is not established"
	return r
}

type junitCase struct {
	Failure *struct{} `xml:"failure"`
	Error   *struct{} `xml:"error"`
	Skipped *struct{} `xml:"skipped"`
}
type junitSuite struct {
	Failures int `xml:"failures,attr"`
	Errors   int `xml:"errors,attr"`
	XMLName  xml.Name
	Cases    []junitCase  `xml:"testcase"`
	Suites   []junitSuite `xml:"testsuite"`
}

const (
	maxXMLBytes  = 8 << 20
	maxXMLDepth  = 128
	maxXMLTokens = 1 << 20
)

func validateXMLDocument(b []byte) error {
	if len(b) == 0 {
		return fmt.Errorf("empty XML document")
	}
	if len(b) > maxXMLBytes {
		return fmt.Errorf("XML document exceeds response budget")
	}
	if bytes.Contains(b, []byte("<!DOCTYPE")) || bytes.Contains(b, []byte("<!ENTITY")) {
		return fmt.Errorf("XML document type declarations are not accepted")
	}
	d := xml.NewDecoder(bytes.NewReader(b))
	depth := 0
	tokens := 0
	for {
		tokens++
		if tokens > maxXMLTokens {
			return fmt.Errorf("XML token budget exceeded")
		}
		t, err := d.Token()
		if err == io.EOF {
			if depth != 0 {
				return fmt.Errorf("XML element depth underflow")
			}
			return nil
		}
		if err != nil {
			return err
		}
		switch t.(type) {
		case xml.StartElement:
			depth++
			if depth > maxXMLDepth {
				return fmt.Errorf("XML element depth exceeds %d", maxXMLDepth)
			}
		case xml.EndElement:
			depth--
			if depth < 0 {
				return fmt.Errorf("XML element depth underflow")
			}
		}
	}
}

func junit(b []byte, min, exit int) Parsed {
	if len(b) == 0 {
		return Parsed{Outcome: "INCONCLUSIVE", Meaning: "JUnit report is missing or empty"}
	}
	if err := validateXMLDocument(b); err != nil {
		return Parsed{Outcome: "ERROR", Meaning: "malformed JUnit XML: " + err.Error()}
	}
	var root junitSuite
	d := xml.NewDecoder(bytes.NewReader(b))
	d.Strict = true
	d.Entity = map[string]string{"amp": "&", "lt": "<", "gt": ">", "quot": "\"", "apos": "'"}
	if e := d.Decode(&root); e != nil {
		return Parsed{Outcome: "ERROR", Meaning: "malformed JUnit XML: " + e.Error()}
	}
	if root.XMLName.Local != "testsuite" && root.XMLName.Local != "testsuites" {
		return Parsed{Outcome: "ERROR", Meaning: "unrecognized JUnit root"}
	}
	for {
		t, e := d.Token()
		if e == io.EOF {
			break
		}
		if e != nil {
			return Parsed{Outcome: "ERROR", Meaning: "malformed trailing XML"}
		}
		if ch, ok := t.(xml.CharData); ok && strings.TrimSpace(string(ch)) == "" {
			continue
		}
		return Parsed{Outcome: "ERROR", Meaning: "trailing JUnit document content"}
	}
	r := Parsed{}
	bad := false
	capped := false
	var visit func(junitSuite)
	visit = func(s junitSuite) {
		if s.Failures > 0 || s.Errors > 0 {
			bad = true
		}
		for _, c := range s.Cases {
			if r.Tests+r.Skipped >= 100000 {
				capped = true
				return
			}
			if c.Skipped != nil {
				r.Skipped++
			} else {
				r.Tests++
			}
			if c.Error != nil || c.Failure != nil {
				bad = true
			}
		}
		for _, sub := range s.Suites {
			visit(sub)
			if capped {
				return
			}
		}
	}
	visit(root)
	if capped {
		return Parsed{Outcome: "ERROR", Meaning: "JUnit testcase budget exceeded"}
	}
	if bad || exit != 0 {
		r.Outcome = "FAIL"
		r.Meaning = "JUnit failure/error or nonzero process exit"
	} else if r.Tests < min {
		r.Outcome = "INCONCLUSIVE"
		r.Meaning = fmt.Sprintf("expected at least %d non-skipped testcases; observed %d", min, r.Tests)
	} else {
		r.Outcome = "PASS"
		r.Meaning = "reported testcases passed; report is produced by repository tooling, not independent proof"
	}
	return r
}
