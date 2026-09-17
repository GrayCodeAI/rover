package store

import (
	"encoding/json"
	"errors"
	"github.com/GrayCodeAI/rover/internal/model"
)

// ErrBudgetExhausted marks a Charge that would exceed the configured cap.
var ErrBudgetExhausted = errors.New("budget exhausted")

// Budget is the local global accounting ledger for agents sharing one store.
// It is persisted under kind=settings (no schema migration) so every process
// opening the same state directory reads the same ledger. This is in-memory
// style accounting for a single store; it is not fleet-wide billing.
type Budget struct {
	Cap     int            `json:"cap"`
	Version int            `json:"version"`
	Use     map[string]int `json:"use"`
}

// BudgetUse returns the current per-owner accounting. cap 0 means no budget is
// configured and Charge never refuses (honest default: not enforced).
func (s *Store) BudgetUse() (Budget, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	b, e := s.budget()
	return *b, e
}

func (s *Store) budget() (*Budget, error) {
	b := &Budget{Version: 0, Use: map[string]int{}}
	e := s.get("settings", "budget", b)
	if errors.Is(e, ErrNotFound) {
		return b, nil
	}
	if e != nil {
		return nil, e
	}
	if b.Cap < 0 || b.Cap > 1<<30 || b.Version < 0 {
		return nil, errors.New("invalid budget ledger")
	}
	if b.Use == nil {
		b.Use = map[string]int{}
	}
	for _, v := range b.Use {
		if v < 0 || v > 1<<30 {
			return nil, errors.New("invalid budget ledger")
		}
	}
	return b, nil
}

// SetBudgetCap configures the global cap. Passing 0 removes enforcement.
// The ledger version increments so concurrent readers can detect external
// changes before acting on stale accounting.
func (s *Store) SetBudgetCap(cap int) error {
	if cap < 0 || cap > 1<<30 {
		return errors.New("invalid budget cap")
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.transaction(func() error {
		b, e := s.budget()
		if e != nil {
			return e
		}
		b.Cap = cap
		b.Version++
		return s.put("settings", "budget", b, "budget.cap")
	})
}

// Charge records owner's spend. When a cap is configured the aggregate is
// enforced atomically and over-limit charges are refused (ErrBudgetExhausted)
// without mutating the ledger.
func (s *Store) Charge(owner string, amount int) error {
	if !model.ValidID(owner) {
		return errors.New("invalid owner")
	}
	if amount < 0 || amount > 1<<30 {
		return errors.New("invalid charge")
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.transaction(func() error {
		b, e := s.budget()
		if e != nil {
			return e
		}
		cur := b.Use[owner]
		var total int64 = int64(b.Total()) + int64(amount)
		if b.Cap > 0 && total > int64(b.Cap) {
			return ErrBudgetExhausted
		}
		b.Use[owner] = cur + amount
		b.Version++
		return s.put("settings", "budget", b, "budget.charge")
	})
}

// BudgetReset zeroes an owner's accounting (or all owners when owner=="*").
func (s *Store) BudgetReset(owner string) error {
	if owner != "*" && !model.ValidID(owner) {
		return errors.New("invalid owner")
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.transaction(func() error {
		b, e := s.budget()
		if e != nil {
			return e
		}
		if owner == "*" {
			b.Use = map[string]int{}
		} else {
			delete(b.Use, owner)
		}
		b.Version++
		return s.put("settings", "budget", b, "budget.reset")
	})
}

// Total sums the ledger across owners.
func (b *Budget) Total() int {
	t := 0
	for _, v := range b.Use {
		t += v
	}
	return t
}

func (b *Budget) String() string {
	j, _ := json.Marshal(b)
	return string(j)
}
