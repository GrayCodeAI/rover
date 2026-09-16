package store

import (
	"errors"
	"path/filepath"
	"testing"
)

// TestGlobalBudgetAccounting covers A08/A09 local ledger semantics: cap
// enforcement, atomic refusal (ledger unchanged on ErrBudgetExhausted), and
// per-owner reset. It runs against a real sqlite store but is strictly a
// single-store local observation; there is no fleet-wide billing.
func TestGlobalBudgetAccounting(t *testing.T) {
	s, e := Open(filepath.Join(t.TempDir(), "state"))
	if e != nil {
		t.Fatal(e)
	}
	defer s.Close()

	if e = s.Charge("agent_a", 5); e != nil {
		t.Fatal(e)
	}
	if b, e := s.BudgetUse(); e != nil || b.Total() != 5 {
		t.Fatalf("want total 5, got %d (err %v)", b.Total(), e)
	}
	if e = s.SetBudgetCap(10); e != nil {
		t.Fatal(e)
	}
	if e = s.Charge("agent_a", 4); e != nil {
		t.Fatal(e)
	}
	// An over-limit charge must be refused and must not mutate the ledger.
	if e = s.Charge("agent_b", 2); !errors.Is(e, ErrBudgetExhausted) {
		t.Fatalf("want ErrBudgetExhausted, got %v", e)
	}
	if b, e := s.BudgetUse(); e != nil {
		t.Fatal(e)
	} else if b.Total() != 9 {
		t.Fatalf("ledger mutated on refused charge: total %d", b.Total())
	}
	if e = s.BudgetReset("agent_a"); e != nil {
		t.Fatal(e)
	}
	if b, e := s.BudgetUse(); e != nil {
		t.Fatal(e)
	} else if b.Total() != 0 {
		t.Fatalf("reset expected 0, got %d", b.Total())
	}
	if e = s.SetBudgetCap(0); e != nil {
		t.Fatal(e)
	}
	// No cap configured means charges are never refused (honest default).
	if e = s.Charge("agent_b", 1<<20); e != nil {
		t.Fatalf("uncapped charge refused: %v", e)
	}
}

// TestBudgetRefusesInvalidInput ensures the accounting ledger rejects
// malformed owners and amounts before any mutation.
func TestBudgetRefusesInvalidInput(t *testing.T) {
	s, e := Open(filepath.Join(t.TempDir(), "state"))
	if e != nil {
		t.Fatal(e)
	}
	defer s.Close()
	for _, bad := range []string{"", "bad/owner", "sp ace", "semi;colon"} {
		if e = s.Charge(bad, 1); e == nil {
			t.Fatalf("expected error for owner %q", bad)
		}
	}
	if e = s.Charge("agent_a", -1); e == nil {
		t.Fatal("expected error for negative charge")
	}
	if e = s.SetBudgetCap(-5); e == nil {
		t.Fatal("expected error for negative cap")
	}
}
