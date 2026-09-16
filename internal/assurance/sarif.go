package assurance

import (
	"bytes"
	"encoding/json"
	"fmt"
	"github.com/GrayCodeAI/rover/internal/config"
	"github.com/GrayCodeAI/rover/internal/model"
	"io"
	"net/url"
	"strings"
)

func sarif(b []byte, exit int, threshold string) Parsed {
	if len(b) == 0 {
		return Parsed{Outcome: "INCONCLUSIVE", Meaning: "SARIF report missing"}
	}
	type Result struct {
		RuleID  string `json:"ruleId"`
		Level   string `json:"level"`
		Message struct {
			Text string `json:"text"`
		} `json:"message"`
		Locations []struct {
			Physical struct {
				Artifact struct {
					URI string `json:"uri"`
				} `json:"artifactLocation"`
				Region struct {
					StartLine int `json:"startLine"`
				} `json:"region"`
			} `json:"physicalLocation"`
		} `json:"locations"`
	}
	var doc struct {
		Version string `json:"version"`
		Runs    []struct {
			Tool struct {
				Driver struct {
					Name string `json:"name"`
				} `json:"driver"`
			} `json:"tool"`
			Invocations []struct {
				Success *bool `json:"executionSuccessful"`
			} `json:"invocations"`
			Results *[]Result `json:"results"`
		} `json:"runs"`
	}
	d := json.NewDecoder(bytes.NewReader(b))
	if d.Decode(&doc) != nil || d.Decode(new(any)) != io.EOF {
		return Parsed{Outcome: "ERROR", Meaning: "malformed/trailing SARIF document"}
	}
	if doc.Version != "2.1.0" || len(doc.Runs) == 0 {
		return Parsed{Outcome: "INCONCLUSIVE", Meaning: "SARIF 2.1.0 and nonempty runs required"}
	}
	ranks := map[string]int{"none": 0, "note": 1, "warning": 2, "error": 3}
	if threshold == "" {
		threshold = "warning"
	}
	rank, ok := ranks[threshold]
	if !ok || rank == 0 {
		return Parsed{Outcome: "ERROR", Meaning: "invalid SARIF threshold"}
	}
	r := Parsed{Outcome: "PASS", Meaning: "no reported findings at configured severity; not proof of security"}
	for _, run := range doc.Runs {
		if strings.TrimSpace(run.Tool.Driver.Name) == "" || run.Results == nil {
			return Parsed{Outcome: "INCONCLUSIVE", Meaning: "tool identity or explicit results array absent"}
		}
		for _, inv := range run.Invocations {
			if inv.Success != nil && !*inv.Success {
				return Parsed{Outcome: "ERROR", Meaning: "analyzer reported unsuccessful execution"}
			}
		}
		for _, x := range *run.Results {
			if len(r.Findings) >= 10000 {
				return Parsed{Outcome: "ERROR", Meaning: "diagnostic budget exceeded"}
			}
			lev := x.Level
			if lev == "" {
				lev = "warning"
			}
			n, ok := ranks[lev]
			if !ok || strings.TrimSpace(x.Message.Text) == "" {
				return Parsed{Outcome: "ERROR", Meaning: "invalid diagnostic severity/message"}
			}
			f := model.Finding{Rule: x.RuleID, Message: x.Message.Text, Severity: lev, Source: "sarif:" + run.Tool.Driver.Name}
			if f.Rule == "" {
				f.Rule = "unspecified-rule"
			}
			if len(x.Locations) > 0 {
				l := x.Locations[0].Physical
				u, e := url.Parse(l.Artifact.URI)
				if e == nil && u.Scheme == "" && u.Host == "" {
					p, e := url.PathUnescape(u.Path)
					if e == nil && config.Relative(p) {
						f.Path = p
						f.Line = l.Region.StartLine
					}
				}
			}
			r.Findings = append(r.Findings, f)
			if n >= rank {
				r.Outcome = "FAIL"
			}
		}
	}
	if exit != 0 {
		r.Outcome = "ERROR"
		r.Meaning = "analyzer exited nonzero; tool-specific exit semantics are not assumed"
	} else if r.Outcome == "FAIL" {
		r.Meaning = fmt.Sprintf("reported findings meet severity threshold %s", threshold)
	}
	return r
}
