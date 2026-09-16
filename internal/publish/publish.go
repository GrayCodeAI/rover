package publish

import (
	"encoding/json"
	"errors"
	"os"
	"path/filepath"

	"github.com/GrayCodeAI/rover/internal/model"
	"github.com/GrayCodeAI/rover/internal/store"
)

// Publish writes a local, advisory copy of an investigation to destDir.
// It is not a protected CI publisher — it is a local file export that can
// be inspected, not an independent acceptance gate.
func Publish(s *store.Store, investigationID, destDir string) (string, error) {
	if !model.ValidID(investigationID) {
		return "", errors.New("invalid investigation ID")
	}
	var inv model.Investigation
	if e := s.Get("investigation", investigationID, &inv); e != nil {
		return "", e
	}
	if e := os.MkdirAll(destDir, 0700); e != nil {
		return "", e
	}
	// Ensure dest is not inside state (avoid recursion) and is a directory.
	if store.IsWithin(s.Root, destDir) {
		return "", errors.New("publish destination must be outside state")
	}
	data, e := json.MarshalIndent(inv, "", "  ")
	if e != nil {
		return "", e
	}
	out := filepath.Join(destDir, investigationID+".json")
	if e = os.WriteFile(out, data, 0600); e != nil {
		return "", e
	}
	return out, nil
}
