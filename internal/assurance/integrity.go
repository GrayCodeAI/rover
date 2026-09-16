package assurance

import (
	"fmt"
	"github.com/GrayCodeAI/rover/internal/model"
	"github.com/GrayCodeAI/rover/internal/source"
	"github.com/GrayCodeAI/rover/internal/store"
	"regexp"
)

var skipPattern = regexp.MustCompile(`(?m)(@pytest\.mark\.(?:skip|xfail)|\b(?:it|describe|test)\.skip\s*\(|\bt\.Skip(?:f|Now)?\s*\(|#\[ignore\])`)
var assertionPattern = regexp.MustCompile(`(?m)(\bassert\b|\bassert_eq!|\bassert!|\bexpect\s*\(|\bt\.Fatal(?:f)?\s*\()`)

func TestIntegrity(s *store.Store, base, cand model.Snapshot) ([]model.Finding, error) {
	out := []model.Finding{}
	for _, ch := range source.Compare(base, cand).Changes {
		if ch.Category != "test" || ch.Status == "deleted" {
			continue
		}
		b, e := source.FileContent(s, base, ch.Path)
		if e != nil && e != store.ErrNotFound {
			return nil, e
		}
		n, e := source.FileContent(s, cand, ch.Path)
		if e != nil {
			return nil, e
		}
		oldSk, newSk := len(skipPattern.FindAll(b, -1)), len(skipPattern.FindAll(n, -1))
		if newSk > oldSk {
			out = append(out, model.Finding{Rule: "possible_test_skip_added", Path: ch.Path, Source: "lexical_heuristic", Severity: "warning", Message: fmt.Sprintf("skip/xfail markers increased %d to %d; inspect semantics", oldSk, newSk)})
		}
		oldAs, newAs := len(assertionPattern.FindAll(b, -1)), len(assertionPattern.FindAll(n, -1))
		if oldAs > newAs {
			out = append(out, model.Finding{Rule: "possible_assertion_removed", Path: ch.Path, Source: "lexical_heuristic", Severity: "warning", Message: fmt.Sprintf("assertion-like markers decreased %d to %d; not proof of weakened tests", oldAs, newAs)})
		}
	}
	return out, nil
}
